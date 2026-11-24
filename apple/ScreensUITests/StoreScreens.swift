import XCTest

#if os(iOS)
import UIKit
#endif

/// The App Store's pictures: the same few screens in each of the app's six
/// languages, light, with the store sample library (-PaperloomStore: no
/// test plugins, a paper with a readable PDF and highlights). Run by the
/// Xcode Cloud workflow "Store Screens" (docs/ci.md) on an iPhone 17 Pro
/// Max (6.9-inch) and an iPad Pro 13-inch simulator, and on a Mac, where the
/// app's window is 1280 × 800 and only the window is photographed. The
/// pictures are attachments named store-<device>-<language>-<n>-<screen>.
final class StoreScreens: XCTestCase {
    private var app: XCUIApplication!
    private var device = ""
    private var language = "en"
    private var shots = 0
    private var missing: [String] = []

    @MainActor func testEnglish() { walk("en", "en_US") }
    @MainActor func testSimplifiedChinese() { walk("zh-Hans", "zh_CN") }
    @MainActor func testTraditionalChinese() { walk("zh-Hant", "zh_TW") }
    @MainActor func testFrench() { walk("fr", "fr_FR") }
    @MainActor func testJapanese() { walk("ja", "ja_JP") }
    @MainActor func testRussian() { walk("ru", "ru_RU") }

    @MainActor
    private func walk(_ language: String, _ locale: String) {
        continueAfterFailure = true
        self.language = language
        app = XCUIApplication()
        app.launchArguments += [
            "-PaperloomScreenshots", "-PaperloomStore", "-PaperloomAppearance", "light",
            "-AppleLanguages", "(\(language))", "-AppleLocale", locale,
        ]
        #if os(iOS)
        let phone = UIDevice.current.userInterfaceIdiom == .phone
        device = phone ? "iphone" : "ipad"
        XCUIDevice.shared.orientation = phone ? .portrait : .landscapeLeft
        #else
        let phone = false
        device = "mac"
        #endif
        app.launch()
        waitFor("Attention Is All You Need")
        if phone {
            phoneScreens()
        } else {
            workbenchScreens()
        }
        XCTAssertTrue(missing.isEmpty, "Not reached: \(missing.joined(separator: "; "))")
    }

    /// iPhone: the library, a paper, its PDF, a note, then search.
    @MainActor
    private func phoneScreens() {
        shoot("library")
        if press("Attention Is All You Need", partly: true, types: [.cell, .button, .staticText]) {
            shoot("item")
            if press(l("Read"), types: [.button, .link]) {
                shoot("reader", settle: 3)
                back()
            }
            back()
        }
        if tab(l("Notes")), press("Transformers", partly: true, types: [.button, .cell, .staticText]) {
            // The rendered note loads in a web view.
            shoot("note", settle: 5)
            back()
        }
        if tab(nil) { search("search") }
    }

    /// iPad and Mac: the workbench with its sidebar and document tabs.
    @MainActor
    private func workbenchScreens() {
        shoot("library")
        // On the Mac the library's rows are buttons.
        if press("Attention Is All You Need", partly: true, types: rowTypes + [.button]) {
            shoot("item")
            if press(l("Read"), types: [.button, .link]) { shoot("reader", settle: 3) }
        }
        if press(l("Notes"), types: rowTypes),
           press("Transformers", partly: true, types: rowTypes + [.button]) {
            shoot("note", settle: 5)
        }
        if press(l("Search Everything"), types: rowTypes) { search("search") }
        if press(l("Online Search"), types: rowTypes) { shoot("online-search") }
    }

    /// Searches the library for "attention" and photographs what it finds.
    @MainActor
    private func search(_ name: String) {
        let field = app.searchFields.firstMatch.exists ? app.searchFields.firstMatch : app.textFields.firstMatch
        guard field.waitForExistence(timeout: 5) else {
            missing.append("\(device) \(language) search field")
            return
        }
        click(field)
        // Return searches, and puts the keyboard away.
        field.typeText("attention\n")
        shoot(name, settle: 3)
    }

    // MARK: - Helpers

    /// The app's translation of one of its labels, from the copy of its
    /// string catalog in this bundle.
    private func l(_ key: String) -> String {
        guard language != "en",
            let path = Bundle(for: StoreScreens.self).path(forResource: language, ofType: "lproj"),
            let strings = Bundle(path: path)
        else { return key }
        return strings.localizedString(forKey: key, value: key, table: "Localizable")
    }

    /// Sidebar and list rows: cells on iOS, outline and table rows on the
    /// Mac. Not buttons: the toolbar has some of the same names.
    private var rowTypes: [XCUIElement.ElementType] { [.cell, .outlineRow, .tableRow, .staticText] }

    @MainActor
    private func shoot(_ name: String, settle: TimeInterval = 1.5) {
        Thread.sleep(forTimeInterval: settle)
        shots += 1
        #if os(iOS)
        // A landscape picture comes upright only through its orientation
        // flag; App Store Connect wants the pixels turned.
        let shot = XCUIScreen.main.screenshot().image
        let format = UIGraphicsImageRendererFormat()
        format.scale = shot.scale
        let upright = UIGraphicsImageRenderer(size: shot.size, format: format).image { _ in
            shot.draw(in: CGRect(origin: .zero, size: shot.size))
        }
        let image = XCTAttachment(data: upright.pngData() ?? Data(), uniformTypeIdentifier: "public.png")
        #else
        let image = XCTAttachment(screenshot: app.windows.firstMatch.screenshot())
        #endif
        image.name = "store-\(device)-\(language)-\(shots)-\(name)"
        image.lifetime = .keepAlways
        add(image)
    }

    @MainActor
    private func find(
        _ label: String, partly: Bool, types: [XCUIElement.ElementType], timeout: TimeInterval
    ) -> XCUIElement? {
        let deadline = Date().addingTimeInterval(timeout)
        repeat {
            for type in types {
                let matches = app.descendants(matching: type).matching(Self.matching(label, partly: partly, type: type))
                for index in 0..<min(matches.count, 10) {
                    let element = matches.element(boundBy: index)
                    if element.exists && element.isHittable { return element }
                    #if os(macOS)
                    // Inside some lists the Mac calls a visible row unhittable;
                    // `click` then aims at the middle of its frame.
                    if element.exists && !element.frame.isEmpty { return element }
                    #endif
                }
            }
            Thread.sleep(forTimeInterval: 0.5)
        } while Date() < deadline
        return nil
    }

    /// Elements showing `label`. On the Mac a text's words are its value,
    /// and a button's may be its title; only a text's value is surely a
    /// string, which CONTAINS needs.
    private static func matching(_ label: String, partly: Bool, type: XCUIElement.ElementType) -> NSPredicate {
        let compare = partly ? "CONTAINS" : "=="
        var format = "label \(compare) %@ OR title \(compare) %@ OR identifier == %@"
        var arguments: [Any] = [label, label, label]
        if type == .staticText {
            format += " OR value \(compare) %@"
            arguments.append(label)
        }
        return NSPredicate(format: format, argumentArray: arguments)
    }

    @MainActor
    @discardableResult
    private func press(
        _ label: String, partly: Bool = false, types: [XCUIElement.ElementType] = [.button, .cell, .staticText],
        timeout: TimeInterval = 8
    ) -> Bool {
        guard let element = find(label, partly: partly, types: types, timeout: timeout) else {
            missing.append("\(device) \(language) \(label) (after shot \(shots))")
            // What was on screen, to see what the label belongs to.
            let tree = XCTAttachment(string: app.debugDescription)
            tree.name = "store-\(device)-\(language)-missing-\(shots).txt"
            tree.lifetime = .keepAlways
            add(tree)
            return false
        }
        click(element)
        return true
    }

    @MainActor
    private func click(_ element: XCUIElement) {
        #if os(iOS)
        element.tap()
        #else
        element.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5)).click()
        #endif
    }

    @MainActor
    private func waitFor(_ label: String) {
        if find(label, partly: true, types: [.cell, .outlineRow, .button, .staticText], timeout: 30) == nil {
            missing.append("\(device) \(language) \(label) never appeared")
        }
    }

    /// A tab in the iPhone's tab bar; nil for the search tab, the last.
    @MainActor
    private func tab(_ label: String?) -> Bool {
        let tabs = app.tabBars.buttons
        let button = label.map { tabs[$0].firstMatch } ?? tabs.element(boundBy: max(tabs.count - 1, 0))
        guard button.waitForExistence(timeout: 5) else {
            missing.append("\(device) \(language) tab \(label ?? "search")")
            return false
        }
        click(button)
        return true
    }

    /// The navigation bar's back button (iPhone).
    @MainActor
    private func back() {
        let button = app.navigationBars.buttons.element(boundBy: 0)
        if button.waitForExistence(timeout: 3) {
            click(button)
        } else {
            missing.append("\(device) \(language) back button (after shot \(shots))")
        }
    }
}
