import Foundation

/// The complete item projection, including trash. Independent of views,
/// the core and actors so the web transition fixtures can exercise it directly.
struct PluginItemSnapshot {
    let items: [[String: Any]]
    let byKey: [String: [String: Any]]
    private let fieldsByKey: [String: [String]]

    init(json: String) throws {
        guard let records = try JSONSerialization.jsonObject(with: Data(json.utf8)) as? [[String: Any]] else {
            throw PluginItemError.invalidProjection
        }
        var items: [[String: Any]] = []
        var byKey: [String: [String: Any]] = [:]
        var fieldsByKey: [String: [String]] = [:]
        let orders = Self.propertyOrders(in: json)
        for (index, record) in records.enumerated() {
            var item = record
            guard let key = item["key"] as? String, byKey[key] == nil else {
                throw PluginItemError.invalidProjection
            }
            var fields = orders[index]
            if item["deleted"] == nil { fields.append("deleted") }
            item["deleted"] = item["deleted"] as? Bool ?? false
            if let tags = item["tags"] as? [Any] {
                item["tags"] = tags.sorted {
                    String(decoding: Self.normalized($0), as: UTF8.self)
                        .localizedCompare(String(decoding: Self.normalized($1), as: UTF8.self)) == .orderedAscending
                }
            }
            items.append(item)
            byKey[key] = item
            fieldsByKey[key] = fields
        }
        // Flat attachment and annotation rows share the regular-item lifecycle.
        // Reject an incomplete child projection rather than producing false deletes.
        let annotationTypes: Set<String> = ["highlight", "underline", "note", "ink", "text"]
        for item in items where item["itemType"] as? String == "annotation" {
            guard let parent = item["parentItem"] as? String,
                  byKey[parent]?["itemType"] as? String == "attachment",
                  let type = item["annotationType"] as? String,
                  annotationTypes.contains(type) else {
                throw PluginItemError.invalidProjection
            }
        }
        self.items = items
        self.byKey = byKey
        self.fieldsByKey = fieldsByKey
    }

    /// Object property order and tag order are immaterial; creator order is not.
    static func normalized(_ value: Any?) -> Data {
        // Missing and null fields compare equally, as in the web host.
        (try? JSONSerialization.data(withJSONObject: value ?? NSNull(), options: [.sortedKeys, .fragmentsAllowed]))
            ?? Data("null".utf8)
    }

    func events(since before: PluginItemSnapshot?) -> [PluginItemEvent] {
        guard let before else { return [] } // First successful read is a baseline.
        var groups = ["add", "modify", "trash", "delete"].map { PluginItemEvent(event: $0) }
        for item in items {
            guard let key = item["key"] as? String else { continue } // Validated at construction.
            guard let old = before.byKey[key] else {
                groups[0].itemKeys.append(key)
                continue
            }
            var seen = Set<String>()
            let fields = ((before.fieldsByKey[key] ?? []) + (fieldsByKey[key] ?? [])).filter {
                seen.insert($0).inserted && $0 != "key" && Self.normalized(old[$0]) != Self.normalized(item[$0])
            }
            if old["deleted"] as? Bool != true && item["deleted"] as? Bool == true {
                groups[2].itemKeys.append(key)
            } else if !fields.isEmpty {
                // Restoring an item is modify, preserving its runtime identity.
                groups[1].itemKeys.append(key)
                groups[1].changed[key] = fields
            }
        }
        for item in before.items {
            if let key = item["key"] as? String, byKey[key] == nil { groups[3].itemKeys.append(key) }
        }
        return groups.flatMap { group in
            stride(from: 0, to: group.itemKeys.count, by: 500).map { start in
                group.restricted(to: Array(group.itemKeys[start..<min(start + 500, group.itemKeys.count)]))
            }
        }
    }

    /// Foundation dictionaries discard JSON property order. Keep just the
    /// top-level names so `changed` follows the web's old-then-new field order.
    /// JSONSerialization has already validated the entire input before this scan.
    private static func propertyOrders(in json: String) -> [[String]] {
        let bytes = Array(json.utf8)
        let whitespace: Set<UInt8> = [0x20, 0x09, 0x0a, 0x0d]
        var orders: [[String]] = []
        var depth = 0
        var index = 0
        while index < bytes.count {
            switch bytes[index] {
            case 0x22: // Quoted string; skip escaped quotes and nested content.
                let start = index
                index += 1
                while index < bytes.count {
                    if bytes[index] == 0x5c { index += 2 }
                    else if bytes[index] == 0x22 { break }
                    else { index += 1 }
                }
                if depth == 2 {
                    var next = index + 1
                    while next < bytes.count && whitespace.contains(bytes[next]) { next += 1 }
                    if next < bytes.count, bytes[next] == 0x3a,
                       let key = try? JSONSerialization.jsonObject(with: Data(bytes[start...index]), options: .fragmentsAllowed) as? String {
                        orders[orders.count - 1].append(key)
                    }
                }
            case 0x7b: // Object.
                if depth == 1 { orders.append([]) }
                depth += 1
            case 0x5b: depth += 1 // Array.
            case 0x7d, 0x5d: depth -= 1
            default: break
            }
            index += 1
        }
        return orders
    }
}

struct PluginItemEvent {
    let event: String
    var itemKeys: [String] = []
    var changed: [String: [String]] = [:]

    func restricted(to keys: [String]) -> PluginItemEvent {
        var fields: [String: [String]] = [:]
        for key in keys { fields[key] = changed[key] }
        return PluginItemEvent(event: event, itemKeys: keys, changed: fields)
    }

    var message: [String: Any] {
        var message: [String: Any] = ["event": event, "objectType": "item", "itemKeys": itemKeys]
        if event == "modify" { message["changed"] = changed }
        return message
    }
}

enum PluginItemError: LocalizedError {
    case invalidProjection, unavailable, alternatingWrites, invalidSelection, unknownItem, selectionUnavailable, unknownTab

    var errorDescription: String? {
        switch self {
        case .invalidProjection: "Invalid Zotero item projection."
        case .unavailable: "Could not read the library for Zotero plugins."
        case .alternatingWrites: "Repeated alternating plugin item writes stopped; possible plugin write loop."
        case .invalidSelection: "Invalid item selection."
        case .unknownItem: "Cannot select an unknown item."
        case .selectionUnavailable: "Library selection is unavailable."
        case .unknownTab: "Cannot select an unknown tab."
        }
    }
}

/// A PDF tab as Zotero plugins see it: an opaque view id that lasts while
/// the tab is open, its attachment and that attachment's item.
struct PluginReaderView: Equatable {
    let viewID: String
    let attachmentKey: String
    let parentItemKey: String

    var message: [String: Any] {
        ["viewID": viewID, "attachmentKey": attachmentKey, "parentItemKey": parentItemKey]
    }
}
