import Foundation

/// Editing commands for Markdown notes, as pure functions of the text and
/// the selection (character offsets), matching the web toolbar.
enum MarkdownEditing {
    struct Edit: Equatable {
        var text: String
        /// Character offsets of the new selection.
        var selection: Range<Int>
    }

    private static func splice(_ text: String, _ range: Range<Int>, _ insert: String) -> String {
        let characters = Array(text)
        return String(characters[..<range.lowerBound]) + insert + String(characters[range.upperBound...])
    }

    private static func slice(_ text: String, _ range: Range<Int>) -> String {
        let characters = Array(text)
        let lower = max(0, min(range.lowerBound, characters.count))
        let upper = max(lower, min(range.upperBound, characters.count))
        return String(characters[lower..<upper])
    }

    /// Wraps the selection in markers, or unwraps it if it already is.
    static func toggleWrap(_ text: String, _ selection: Range<Int>, before: String, after: String? = nil) -> Edit {
        let after = after ?? before
        let selected = slice(text, selection)
        if selected.count >= before.count + after.count, selected.hasPrefix(before), selected.hasSuffix(after) {
            let inner = String(selected.dropFirst(before.count).dropLast(after.count))
            return Edit(text: splice(text, selection, inner), selection: selection.lowerBound..<selection.lowerBound + inner.count)
        }
        let outside = (selection.lowerBound - before.count)..<(selection.upperBound + after.count)
        if outside.lowerBound >= 0, slice(text, outside) == before + selected + after {
            return Edit(text: splice(text, outside, selected), selection: outside.lowerBound..<outside.lowerBound + selected.count)
        }
        let start = selection.lowerBound + before.count
        return Edit(text: splice(text, selection, before + selected + after), selection: start..<start + selected.count)
    }

    /// Puts a prefix before every line the selection touches, or takes it
    /// away if all of them have it; a heading prefix replaces another level.
    static func toggleLinePrefix(_ text: String, _ selection: Range<Int>, prefix: String) -> Edit {
        let characters = Array(text)
        var start = min(selection.lowerBound, characters.count)
        while start > 0 && characters[start - 1] != "\n" { start -= 1 }
        var end = min(selection.upperBound, characters.count)
        while end < characters.count && characters[end] != "\n" { end += 1 }
        let lines = String(characters[start..<end]).components(separatedBy: "\n")
        let heading = prefix.allSatisfy { $0 == "#" || $0 == " " } && prefix.hasSuffix(" ")
        let all = lines.allSatisfy { $0.hasPrefix(prefix) }
        let changed = lines.map { line -> String in
            if all { return String(line.dropFirst(prefix.count)) }
            if heading {
                let level = line.prefix { $0 == "#" }.count
                if (1...6).contains(level) && line.dropFirst(level).hasPrefix(" ") {
                    return prefix + line.dropFirst(level + 1)
                }
            }
            return prefix + line
        }.joined(separator: "\n")
        return Edit(text: splice(text, start..<end, changed), selection: start..<start + changed.count)
    }

    /// Colored text as inline HTML (shown by Obsidian, Typora and GitHub).
    static func color(_ text: String, _ selection: Range<Int>, hex: String, background: Bool) -> Edit {
        let selected = slice(text, selection)
        let open = background ? "<mark style=\"background: \(hex)\">" : "<span style=\"color: \(hex)\">"
        let close = background ? "</mark>" : "</span>"
        let start = selection.lowerBound + open.count
        return Edit(text: splice(text, selection, open + selected + close), selection: start..<start + selected.count)
    }

    /// `[[selection]]`, the cursor inside.
    static func wikiLink(_ text: String, _ selection: Range<Int>) -> Edit {
        let selected = slice(text, selection)
        let start = selection.lowerBound + 2
        return Edit(text: splice(text, selection, "[[\(selected)]]"), selection: start..<start + selected.count)
    }

    /// Inserts text in place of the selection; the cursor goes `cursor`
    /// characters in.
    static func insert(_ text: String, _ selection: Range<Int>, _ insert: String, cursor: Int? = nil) -> Edit {
        let at = selection.lowerBound + (cursor ?? insert.count)
        return Edit(text: splice(text, selection, insert), selection: at..<at)
    }

    static let table = "| Column | Column |\n| --- | --- |\n|  |  |\n"
    static let textColors = ["#e03e3e", "#d9730d", "#0f7b6c", "#0b6e99", "#6940a5"]
    static let backgroundColors = ["#fff3a3", "#ffd8b8", "#d3f5d0", "#d7e9fb", "#f5d9f0"]
}
