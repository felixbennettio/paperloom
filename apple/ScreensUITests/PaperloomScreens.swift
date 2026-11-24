import UIKit
import XCTest

/// Walks the main screens on an iPhone or an iPad simulator and keeps a
/// screenshot of each, with the accessibility tree (element frames) beside
/// it, for the Xcode Cloud workflow "UI Screens" (docs/ci.md): once in
/// English, once in Simplified Chinese in dark mode. The app starts with
/// sample papers and notes (-PaperloomScreenshots). A screen it cannot
/// reach is reported at the end, after the others have been photographed.
final class PaperloomScreens: XCTestCase {
    private var app: XCUIApplication!
    /// Starts each screenshot's name: the device, then the language and
    /// appearance when not English and light.
    private var device = ""
    private var language = "en"
    private var shots = 0
    private var missing: [String] = []
    /// What the accessibility audits found, kept as a report rather than
    /// failing the walk.
    private var audits: [String] = []

    @MainActor
    func testScreens() {
        walk(language: "en", locale: "en_US", dark: false)
    }

    @MainActor
    func testScreensInChineseDark() {
        walk(language: "zh-Hans", locale: "zh_CN", dark: true)
    }

    @MainActor
    private func walk(language: String, locale: String, dark: Bool) {
        continueAfterFailure = true
        self.language = language
        app = XCUIApplication()
        app.launchArguments += ["-PaperloomScreenshots", "-AppleLanguages", "(\(language))", "-AppleLocale", locale]
        // Set in the app: the simulator's own setting is not reliable under xcodebuild.
        if dark { app.launchArguments += ["-PaperloomAppearance", "dark"] }
        let pad = UIDevice.current.userInterfaceIdiom == .pad
        device = (pad ? "ipad" : "iphone") + (language == "en" ? "" : "-zh") + (dark ? "-dark" : "")
        if pad {
            padScreens()
        } else {
            phoneScreens()
        }
        if !audits.isEmpty {
            let report = XCTAttachment(string: audits.joined(separator: "\n"))
            report.name = "\(device)-accessibility.txt"
            report.lifetime = .keepAlways
            add(report)
        }
        XCTAssertTrue(missing.isEmpty, "Not reached: \(missing.joined(separator: "; "))")
    }

    /// The app's translation of one of its labels for this walk's language,
    /// from the copy of its string catalog in this bundle.
    private func l(_ key: String) -> String {
        guard language != "en",
            let path = Bundle(for: PaperloomScreens.self).path(forResource: language, ofType: "lproj"),
            let strings = Bundle(path: path)
        else { return key }
        return strings.localizedString(forKey: key, value: key, table: "Localizable")
    }

    @MainActor
    private func phoneScreens() {
        XCUIDevice.shared.orientation = .portrait
        app.launch()
        // Newest first: the first sample paper is at the bottom.
        waitFor("The Structure of Scientific Revolutions")
        shoot("library")
        if language == "en" {
            audit("library")
            expect("Decade: ", "sample Zotero plugin's column")
            openFiles()
        }

        if tap("Thesis", types: [.button]) {
            shoot("collection")
            if tap("Attention Is All You Need", partly: true, types: [.cell, .button, .staticText]) {
                shoot("item")
                if language == "en" { audit("item") }
                app.swipeUp()
                shoot("item-more")
                app.swipeUp()
                shoot("item-end")
                if language == "en" { expect("Sample Section", "sample Zotero plugin's section") }
                back()
            }
            tap(l("All Items"), types: [.button])
        }
        if tap(l("Add")) {
            shoot("add-menu")
            if tap(l("Add Item")) {
                shoot("add-item")
                tap(l("Cancel"))
            }
        }
        if tap(l("Show")) {
            shoot("show-menu")
            if tap(l("Starred")) {
                shoot("starred")
                if tap(l("Show")) { tap(l("All Items")) }
            }
        }

        if tab(l("Reading")) { shoot("reading") }
        if tab(l("Notes")) {
            shoot("notes")
            if tap("Transformers", partly: true, types: [.button, .cell, .staticText]) {
                // The rendered note loads in a web view.
                shoot("note", settle: 5)
                if language == "en" {
                    audit("note")
                    pluginSuggestions()
                }
                back()
            }
        }
        if tab(l("Settings")) {
            shoot("settings")
            if language == "en" { audit("settings") }
            for page in 1...4 {
                app.swipeUp()
                shoot("settings-\(page)")
            }
        }
        if tab(nil) {
            shoot("search")
            let field = app.searchFields.firstMatch
            if field.waitForExistence(timeout: 3) {
                field.tap()
                // Return searches, and puts the keyboard away.
                field.typeText("attention\n")
                shoot("search-results", settle: 3)
                // Words only in the picture of a scanned page: found once
                // text recognition has read it.
                if language == "en", field.waitForExistence(timeout: 3) {
                    field.tap()
                    field.typeText(String(repeating: XCUIKeyboardKey.delete.rawValue, count: 12) + "sensing\n")
                    // The scanned paper's own row; its title has neither word.
                    expect("Scanned Thermometry", "a word read from a scanned page")
                    shoot("search-scanned", settle: 2)
                    field.tap()
                    field.typeText(String(repeating: XCUIKeyboardKey.delete.rawValue, count: 12) + "金刚石\n")
                    expect("Scanned Thermometry", "Chinese read from a scanned page")
                    shoot("search-scanned-chinese", settle: 2)
                }
            }
        }
        if tab(l("Library")) {
            if language == "en" { recognizeScan() }
            XCUIDevice.shared.orientation = .landscapeLeft
            shoot("library-landscape")
            XCUIDevice.shared.orientation = .portrait
        }
    }

    /// The scanned sample paper in the reader: Edit Pages → Recognize Text
    /// writes the words read from its picture into the PDF as invisible text.
    @MainActor
    private func recognizeScan() {
        guard tap("Scanned Thermometry", partly: true, types: [.cell, .button, .staticText]) else { return }
        if tap("Read", types: [.button, .link]) {
            shoot("scan-reader", settle: 3)
            // The menu sits in the toolbar, or in its overflow on a narrow screen.
            var menu = find("Edit Pages", partly: false, types: [.button], timeout: 4)
            if menu == nil, let more = find("More", partly: false, types: [.button], timeout: 2) {
                more.tap()
                menu = find("Edit Pages", partly: false, types: [.button], timeout: 4)
            }
            if let menu {
                menu.tap()
                if tap("Recognize Text", types: [.button]) {
                    expect("Pages with recognized text: 1.", "the scanned page's text written into the PDF")
                    shoot("scan-recognized", settle: 2)
                }
            } else {
                missing.append("\(device) Edit Pages menu in the reader")
                shoot("scan-no-edit-menu")
            }
            back()
        }
        back()
    }

    @MainActor
    private func padScreens() {
        XCUIDevice.shared.orientation = .landscapeLeft
        app.launch()
        waitFor("Attention Is All You Need")
        shoot("library")
        if language == "en" {
            audit("library")
            expect("Decade: ", "sample Zotero plugin's column")
            openFiles()
        }

        if tap("Attention Is All You Need", partly: true, types: [.button, .cell, .staticText]) {
            shoot("item")
            if language == "en" {
                audit("item")
                expect("Sample Section", "sample Zotero plugin's section")
            }
        }
        for (label, name) in [
            (l("Search Everything"), "search-everything"), (l("Online Search"), "online-search"),
            (l("Browser"), "browser"), (l("Notes"), "notes"), (l("Starred"), "starred"),
        ] {
            if tap(label, types: [.cell, .staticText]) { shoot(name, settle: name == "notes" ? 5 : 1.5) }
        }
        if tap(l("Add Item"), types: [.button]) {
            shoot("add-item")
            tap(l("Cancel"))
        }
        if tap(l("Settings"), types: [.button]) {
            shoot("settings")
            app.swipeUp()
            shoot("settings-more")
        }
        XCUIDevice.shared.orientation = .portrait
        shoot("settings-portrait")
        // The tab strip's Library tab; the sidebar is hidden in portrait.
        if tap(l("Library"), types: [.button]) { shoot("library-portrait") }
        XCUIDevice.shared.orientation = .landscapeLeft
    }

    /// "Open in Paperloom": a RIS file and a PDF handed to the app as
    /// another app would, each reported in a sheet and then in the library.
    /// Opening a URL launches the app again, with a fresh sample library,
    /// so each file is looked for right after it is opened.
    @MainActor
    private func openFiles() {
        let folder = FileManager.default.temporaryDirectory
        let ris = folder.appendingPathComponent("Shared.ris")
        let record = "TY  - JOUR\nTI  - Opened from Another App\nAU  - Tester, Pat\nPY  - 2025\nT2  - Journal of Sharing\nER  - \n"
        let pdf = folder.appendingPathComponent("Shared Paper.pdf")
        do {
            try record.write(to: ris, atomically: true, encoding: .utf8)
            try UIGraphicsPDFRenderer(bounds: CGRect(x: 0, y: 0, width: 612, height: 792)).writePDF(to: pdf) { context in
                context.beginPage()
                let text = "A Shared PDF" as NSString
                text.draw(at: CGPoint(x: 72, y: 72), withAttributes: [.font: UIFont.systemFont(ofSize: 24)])
            }
        } catch {
            missing.append("\(device) sample files (\(error.localizedDescription))")
            return
        }
        // The PDF is named after its file or its first line, "Shared P…" either way.
        for (file, name, title) in [(ris, "open-ris", "Opened from Another App"), (pdf, "open-pdf", "Shared P")] {
            app.open(file)
            guard find("Shared to Paperloom", partly: false, types: [.staticText, .other], timeout: 15) != nil else {
                missing.append("\(device) report after opening \(file.lastPathComponent)")
                shoot(name + "-not-delivered")
                continue
            }
            // Done is greyed out while the file is read.
            let done = app.buttons["Done"].firstMatch
            var waited = 0
            while !(done.exists && done.isEnabled) && waited < 40 {
                Thread.sleep(forTimeInterval: 0.5)
                waited += 1
            }
            shoot(name)
            tap("Done")
            waitFor(title)
            shoot(name + "-library")
        }
        readPDF()
    }

    /// The PDF just opened, in the reader: on the iPhone pushed over the
    /// list, on the iPad in a tab of its own.
    @MainActor
    private func readPDF() {
        let phone = UIDevice.current.userInterfaceIdiom != .pad
        guard tap("Shared P", partly: true, types: [.cell, .button, .staticText]) else { return }
        shoot("pdf-item")
        if tap("Read", types: [.button, .link]) {
            shoot("pdf-reader", settle: 3)
            if phone {
                highlight()
                back()
            }
        }
        if phone {
            back()
        } else {
            // Close the item's and the PDF's tabs, back to the library.
            for _ in 0..<2 {
                if !tap("Close tab", types: [.button], timeout: 3) { break }
            }
            tap("All Items", types: [.cell, .staticText])
        }
    }

    /// The sample Obsidian plugin's suggestions: typing "@pap" in the note
    /// offers "Paperloom", and choosing it writes it into the note.
    @MainActor
    private func pluginSuggestions() {
        let editor = app.textViews.firstMatch
        guard editor.waitForExistence(timeout: 5) else {
            missing.append("\(device) note editor")
            return
        }
        editor.tap()
        editor.typeText(" @pap")
        guard find("Paperloom", partly: false, types: [.button, .cell, .staticText], timeout: 15) != nil else {
            missing.append("\(device) plugin suggestions")
            shoot("plugin-suggest-missing")
            return
        }
        shoot("plugin-suggest")
        if tap("Paperloom", types: [.button, .cell, .staticText]) { shoot("plugin-picked") }
    }

    /// A word of the sample PDF highlighted as a reader would: a long press
    /// selects it (a double tap zooms), then Highlight; the annotation list
    /// shows it.
    @MainActor
    private func highlight() {
        // "Shared" in "A Shared PDF", near the top left of the page.
        app.coordinate(withNormalizedOffset: CGVector(dx: 0.22, dy: 0.29)).press(forDuration: 1.2)
        Thread.sleep(forTimeInterval: 1)
        shoot("pdf-selected")
        guard tap("Highlight", types: [.button]) else { return }
        shoot("pdf-highlight", settle: 2)
        if tap("Annotations", types: [.button]) {
            shoot("pdf-annotations", settle: 2)
            // The panel is a sheet on the iPhone: drag it down by its top.
            let top = app.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.09))
            top.press(forDuration: 0.1, thenDragTo: app.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.95)))
            Thread.sleep(forTimeInterval: 1)
        }
    }

    /// The system's accessibility audit of what is on screen: contrast,
    /// labels, tap areas, clipped text, Dynamic Type. Noted, not failed.
    @MainActor
    private func audit(_ screen: String) {
        do {
            try app.performAccessibilityAudit { issue in
                let element = issue.element.map { "“\($0.label)” \($0.identifier)" } ?? ""
                self.audits.append("\(screen): \(issue.compactDescription) \(element)")
                return true
            }
        } catch {
            audits.append("\(screen): audit did not run (\(error.localizedDescription))")
        }
    }

    // MARK: - Helpers

    /// Notes something missing when nothing labelled with `text` appears,
    /// on screen or scrolled away (plugins answer a little later).
    @MainActor
    private func expect(_ text: String, _ what: String) {
        let found = app.descendants(matching: .any).matching(NSPredicate(format: "label CONTAINS %@", text)).firstMatch
        if !found.waitForExistence(timeout: 20) { missing.append("\(device) \(what) (after shot \(shots))") }
    }

    /// Keeps a screenshot and the accessibility tree of what is on screen.
    @MainActor
    private func shoot(_ name: String, settle: TimeInterval = 1.5) {
        // Let sheets, menus and rotations finish moving.
        Thread.sleep(forTimeInterval: settle)
        shots += 1
        let title = "\(device)-\(shots < 10 ? "0" : "")\(shots)-\(name)"
        let image = XCTAttachment(screenshot: XCUIScreen.main.screenshot())
        image.name = title
        image.lifetime = .keepAlways
        add(image)
        let tree = XCTAttachment(string: app.debugDescription)
        tree.name = title + ".txt"
        tree.lifetime = .keepAlways
        add(tree)
    }

    /// The first element on screen that can be tapped, labelled `label` (or
    /// whose label contains it), trying the element types in order.
    @MainActor
    private func find(
        _ label: String, partly: Bool, types: [XCUIElement.ElementType], timeout: TimeInterval
    ) -> XCUIElement? {
        let predicate =
            partly
            ? NSPredicate(format: "label CONTAINS %@", label)
            : NSPredicate(format: "label == %@ OR identifier == %@", label, label)
        let deadline = Date().addingTimeInterval(timeout)
        repeat {
            for type in types {
                let matches = app.descendants(matching: type).matching(predicate)
                for index in 0..<min(matches.count, 10) {
                    let element = matches.element(boundBy: index)
                    if element.exists && element.isHittable { return element }
                }
            }
            Thread.sleep(forTimeInterval: 0.5)
        } while Date() < deadline
        return nil
    }

    @MainActor
    @discardableResult
    private func tap(
        _ label: String, partly: Bool = false, types: [XCUIElement.ElementType] = [.button, .cell, .staticText, .link],
        timeout: TimeInterval = 6
    ) -> Bool {
        guard let element = find(label, partly: partly, types: types, timeout: timeout) else {
            missing.append("\(device) \(label) (after shot \(shots))")
            return false
        }
        element.tap()
        return true
    }

    /// A tab in the iPhone's tab bar; nil for the search tab, which is
    /// last and named by the system.
    @MainActor
    private func tab(_ label: String?) -> Bool {
        let tabs = app.tabBars.buttons
        if let label, !tabs[label].firstMatch.waitForExistence(timeout: 2) {
            // Scrolling down shrinks the tab bar to the current tab; scrolling
            // back up, or a tap on it, brings the others back.
            app.swipeDown()
            if !tabs[label].firstMatch.waitForExistence(timeout: 2), tabs.count > 0 {
                tabs.element(boundBy: 0).tap()
            }
        }
        let button = label.map { tabs[$0].firstMatch } ?? tabs.element(boundBy: max(tabs.count - 1, 0))
        guard button.waitForExistence(timeout: 5) else {
            missing.append("\(device) tab \(label ?? "search")")
            return false
        }
        button.tap()
        return true
    }

    @MainActor
    private func waitFor(_ label: String) {
        if find(label, partly: true, types: [.cell, .button, .staticText], timeout: 30) == nil {
            missing.append("\(device) \(label) never appeared")
        }
    }

    /// The navigation bar's back button.
    @MainActor
    private func back() {
        let button = app.navigationBars.buttons.element(boundBy: 0)
        if button.waitForExistence(timeout: 3) {
            button.tap()
        } else {
            missing.append("\(device) back button (after shot \(shots))")
        }
    }
}
