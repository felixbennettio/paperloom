import SwiftUI

/// A text field in a form that keeps its name in view. A Mac form puts the
/// name beside the field; on the iPhone and iPad a plain field shows only
/// its placeholder, and once filled only its value, so the name goes beside
/// it there too.
struct NamedField: View {
    let title: LocalizedStringKey
    @Binding var text: String
    var prompt: Text?
    var axis: Axis = .horizontal
    var secure = false

    init(
        _ title: LocalizedStringKey, text: Binding<String>, prompt: Text? = nil, axis: Axis = .horizontal,
        secure: Bool = false
    ) {
        self.title = title
        _text = text
        self.prompt = prompt
        self.axis = axis
        self.secure = secure
    }

    var body: some View {
        #if os(iOS)
        LabeledContent(title) {
            // The name is beside it, so an empty field needs no placeholder.
            field(prompt: prompt ?? Text(verbatim: ""))
                .multilineTextAlignment(axis == .vertical ? .leading : .trailing)
        }
        #else
        field(prompt: prompt)
        #endif
    }

    @ViewBuilder private func field(prompt: Text?) -> some View {
        if secure {
            SecureField(title, text: $text, prompt: prompt)
        } else {
            TextField(title, text: $text, prompt: prompt, axis: axis)
        }
    }
}
