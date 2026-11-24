import XCTest
@testable import CoreKit

final class CoreKitTests: XCTestCase {
    private func newItem(_ title: String, doi: String? = nil) -> NewItem {
        NewItem(
            itemType: "", title: title, authors: ["Curie, Marie", "Pierre Curie", "Henri Becquerel"],
            containerTitle: "Comptes rendus", year: 1903, doi: doi)
    }

    func testAddsAndListsItems() throws {
        let library = try PaperLibrary.inMemory()
        let added = try library.addItem(newItem: newItem("Radioactivity", doi: "https://doi.org/10.1000/RADIO"))
        XCTAssertEqual(added.doi, "10.1000/radio")
        XCTAssertEqual(added.authors, "Curie et al.")
        XCTAssertEqual(added.itemType, "journal-article")
        XCTAssertEqual(try library.items(), [added])
    }

    func testCollections() throws {
        let library = try PaperLibrary.inMemory()
        let thesis = try library.createCollection(name: "Thesis", parentId: nil)
        let chapter = try library.createCollection(name: "Chapter 1", parentId: thesis.id)
        let item = try library.addItem(newItem: newItem("A"))
        try library.addToCollection(collectionId: chapter.id, itemId: item.id)
        XCTAssertEqual(try library.itemsInCollection(collectionId: chapter.id).map(\.id), [item.id])
        XCTAssertEqual(try library.collections().count, 2)
    }

    func testErrorsCarryReadableMessages() throws {
        let library = try PaperLibrary.inMemory()
        XCTAssertThrowsError(try library.addItem(newItem: newItem("  "))) { error in
            XCTAssertEqual(CoreKit.message(for: error), "An item needs a title.")
        }
        _ = try library.addItem(newItem: newItem("A", doi: "10.1000/x"))
        XCTAssertThrowsError(try library.addItem(newItem: newItem("B", doi: "doi:10.1000/X"))) { error in
            XCTAssertTrue(CoreKit.message(for: error).contains("10.1000/x"))
        }
    }

    func testDoiHelpers() {
        XCTAssertEqual(normalizeDoi(input: "DOI: 10.1038/NATURE12373"), "10.1038/nature12373")
        XCTAssertNil(normalizeDoi(input: "not a doi"))
        XCTAssertEqual(findDois(text: "see 10.1000/a and 10.1000/b."), ["10.1000/a", "10.1000/b"])
        XCTAssertEqual(itemTypes().count, 12)
        XCTAssertFalse(coreVersion().isEmpty)
    }

    func testDefaultLibraryOpens() throws {
        let url = try CoreKit.defaultLibraryURL()
        XCTAssertEqual(url.lastPathComponent, "library.sqlite")
    }
}
