// The Android library in `android/`, and the share sheet entries it answers:
// links and text, PDFs, and reference files (RIS, BibTeX, EndNote, PubMed,
// which phones often call octet-stream).
const INTENT_FILTERS: &str = r#"<intent-filter>
    <action android:name="android.intent.action.SEND" />
    <category android:name="android.intent.category.DEFAULT" />
    <data android:mimeType="text/plain" />
</intent-filter>
<intent-filter>
    <action android:name="android.intent.action.SEND" />
    <action android:name="android.intent.action.SEND_MULTIPLE" />
    <category android:name="android.intent.category.DEFAULT" />
    <data android:mimeType="application/pdf" />
    <data android:mimeType="application/x-research-info-systems" />
    <data android:mimeType="application/x-bibtex" />
    <data android:mimeType="text/*" />
    <data android:mimeType="application/xml" />
    <data android:mimeType="application/octet-stream" />
</intent-filter>"#;

fn main() {
    tauri_plugin::Builder::new(&[]).android_path("android").build();
    // Only a `tauri android` build sets the project path; elsewhere this
    // does nothing.
    tauri_plugin::mobile::update_android_manifest(
        "PAPERLOOM SHARE PLUGIN",
        "activity",
        INTENT_FILTERS.to_string(),
    )
    .expect("failed to add the share entries to AndroidManifest.xml");
}
