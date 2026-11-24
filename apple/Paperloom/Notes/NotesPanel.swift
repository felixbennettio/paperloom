import SwiftUI

/// The notes dock beside the workbench: the open vault note, compact.
struct NotesPanel: View {
    var body: some View {
        NoteView(compact: true)
    }
}
