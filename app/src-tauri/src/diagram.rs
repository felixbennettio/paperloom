//! The diagram editor's commands: a page laid out, changes made to the
//! diagram's XML, the shapes on offer, and a page as a picture. The editor
//! itself is the app's own; the core does the rest.

use paperloom_core::{DiagramChange, DiagramChanged, DiagramShape, DiagramView};
use tauri::State;

use crate::CommandResult;
use crate::vault::VaultState;

/// The vault's diagrams, by path.
#[tauri::command]
pub(crate) fn vault_diagrams(state: State<'_, VaultState>) -> CommandResult<Vec<String>> {
    state.get()?.diagrams().map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) fn vault_read_diagram(state: State<'_, VaultState>, path: String) -> CommandResult<String> {
    state.get()?.read_diagram(path).map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) fn vault_write_diagram(
    state: State<'_, VaultState>,
    path: String,
    xml: String,
    svg: Option<String>,
) -> CommandResult<()> {
    state.get()?.write_diagram(path, xml, svg).map_err(|e| e.to_string())
}

/// Makes an empty diagram in a folder; its path.
#[tauri::command]
pub(crate) fn vault_create_diagram(
    state: State<'_, VaultState>,
    folder: String,
    name: String,
) -> CommandResult<String> {
    state.get()?.create_diagram(folder, name).map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) fn diagram_view(xml: String, page: u32) -> CommandResult<DiagramView> {
    paperloom_core::diagram_view(xml, page).map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) fn diagram_change(xml: String, page: u32, change: DiagramChange) -> CommandResult<DiagramChanged> {
    paperloom_core::diagram_change(xml, page, change).map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) fn diagram_shapes() -> Vec<DiagramShape> {
    paperloom_core::diagram_shapes()
}

#[tauri::command]
pub(crate) fn diagram_page_svg(xml: String, page: u32) -> CommandResult<String> {
    paperloom_core::diagram_page_svg(xml, page).map_err(|e| e.to_string())
}

/// Asks where to save a page of a diagram, `name` suggested, and writes
/// it: `svg`, `png` (the picture the editor made, in Base64) or `drawio`
/// (the whole diagram); the path, or none if cancelled.
#[tauri::command]
pub(crate) async fn diagram_export<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    name: String,
    xml: String,
    page: u32,
    format: String,
    png: Option<String>,
) -> CommandResult<Option<String>> {
    #[cfg(desktop)]
    {
        use base64::Engine as _;
        use tauri_plugin_dialog::DialogExt;

        let (kind, bytes) = match (format.as_str(), png) {
            ("svg", _) => {
                ("SVG", paperloom_core::diagram_page_svg(xml, page).map_err(|e| e.to_string())?.into_bytes())
            }
            ("png", Some(png)) => {
                ("PNG", base64::engine::general_purpose::STANDARD.decode(png).map_err(|e| e.to_string())?)
            }
            ("pdf", Some(picture)) => {
                let jpeg =
                    base64::engine::general_purpose::STANDARD.decode(picture).map_err(|e| e.to_string())?;
                ("PDF", paperloom_core::picture_pdf(jpeg).map_err(|e| e.to_string())?)
            }
            ("drawio", _) => ("draw.io", xml.into_bytes()),
            _ => return Err(format!("Diagrams can't be saved as .{format} files")),
        };
        let dialog = app
            .dialog()
            .file()
            .add_filter(kind, &[format.as_str()])
            .set_file_name(format!("{name}.{format}"));
        let Some(target) = crate::answer(move || dialog.blocking_save_file()).await? else { return Ok(None) };
        let target = target.into_path().map_err(|e| e.to_string())?;
        std::fs::write(&target, bytes).map_err(|e| e.to_string())?;
        Ok(Some(target.display().to_string()))
    }
    #[cfg(mobile)]
    {
        let _ = (app, name, xml, page, format, png);
        Err("Pictures are saved on a computer.".into())
    }
}
