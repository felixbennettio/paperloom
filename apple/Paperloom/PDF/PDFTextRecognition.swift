import CoreGraphics
import CoreKit
import Foundation
import PDFKit
import Vision

/// Text for PDF pages that have none: a scanned page is a picture, so its
/// words are read from the picture with the system's text recognition, on
/// this device. Search, the AI and recognising the paper then have text.
enum PDFTextRecognition {
    /// A page with fewer letters than this counts as scanned.
    static let minimumLetters = 20
    /// Pages read from pictures at most per document, so a scanned book
    /// does not keep the device busy for long.
    static let pageLimit = 300

    /// Each page's text: its own, or for scanned pages (up to `pageLimit`
    /// of them) what recognition reads in its picture.
    nonisolated static func pages(of document: PDFDocument, count: Int? = nil) -> [String] {
        var recognized = 0
        return (0..<min(count ?? document.pageCount, document.pageCount)).map { index in
            guard let page = document.page(at: index) else { return "" }
            let own = page.string ?? ""
            guard own.filter({ $0.isLetter }).count < minimumLetters, recognized < pageLimit else { return own }
            recognized += 1
            return recognize(page) ?? own
        }
    }

    /// The words in a page's picture, line by line; nil if none were found.
    nonisolated static func recognize(_ page: PDFPage) -> String? {
        let lines = observations(of: page).compactMap { $0.topCandidates(1).first?.string }
        return lines.isEmpty ? nil : lines.joined(separator: "\n")
    }

    /// The lines in a page's picture with their boxes on the page as shown,
    /// as shares of its size from its top-left corner, for a text layer.
    nonisolated static func lines(of page: PDFPage) -> [RecognizedLine] {
        observations(of: page).compactMap { observation in
            guard let text = observation.topCandidates(1).first?.string, !text.isEmpty else { return nil }
            // Vision's boxes start at the bottom left.
            let box = observation.boundingBox
            return RecognizedLine(
                text: text, x: Double(box.minX), y: Double(1 - box.maxY), width: Double(box.width),
                height: Double(box.height))
        }
    }

    /// The scanned pages of a PDF (up to `pageLimit`) with their lines, and
    /// how many pages looked scanned, including those where recognition
    /// found no words.
    nonisolated static func scannedPages(of url: URL) -> (pages: [RecognizedPage], scanned: Int) {
        let scoped = url.startAccessingSecurityScopedResource()
        defer { if scoped { url.stopAccessingSecurityScopedResource() } }
        guard let document = PDFDocument(url: url) else { return ([], 0) }
        var pages: [RecognizedPage] = []
        var scanned = 0
        for index in 0..<document.pageCount where scanned < pageLimit {
            guard let page = document.page(at: index),
                  (page.string ?? "").filter({ $0.isLetter }).count < minimumLetters
            else { continue }
            scanned += 1
            let found = lines(of: page)
            if !found.isEmpty { pages.append(RecognizedPage(pageIndex: UInt32(index), lines: found)) }
        }
        return (pages, scanned)
    }

    /// What recognition finds in a page's picture.
    nonisolated private static func observations(of page: PDFPage) -> [VNRecognizedTextObservation] {
        guard let picture = picture(of: page) else { return [] }
        let request = VNRecognizeTextRequest()
        request.recognitionLevel = .accurate
        request.usesLanguageCorrection = true
        let supported = (try? request.supportedRecognitionLanguages()) ?? []
        request.recognitionLanguages = languages.filter { supported.contains($0) }
        do {
            try VNImageRequestHandler(cgImage: picture, options: [:]).perform([request])
        } catch {
            return []
        }
        return request.results ?? []
    }

    /// The person's own language first, with English, which papers mix in;
    /// Simplified Chinese (whose model also reads English) when theirs is
    /// English or Chinese.
    nonisolated static var languages: [String] {
        let preferred = Locale.preferredLanguages.first?.lowercased() ?? "en"
        let own: String
        switch preferred {
        case let code where code.hasPrefix("zh-hant") || code.hasPrefix("zh-tw") || code.hasPrefix("zh-hk"):
            own = "zh-Hant"
        case let code where code.hasPrefix("ja"): own = "ja-JP"
        case let code where code.hasPrefix("ko"): own = "ko-KR"
        case let code where code.hasPrefix("fr"): own = "fr-FR"
        case let code where code.hasPrefix("ru"): own = "ru-RU"
        case let code where code.hasPrefix("de"): own = "de-DE"
        case let code where code.hasPrefix("es"): own = "es-ES"
        default: own = "zh-Hans"
        }
        return [own, "en-US"]
    }

    /// The page drawn on white at about 200 dots per inch, no more than
    /// 3000 pixels on its long side.
    nonisolated private static func picture(of page: PDFPage) -> CGImage? {
        let box = page.bounds(for: .mediaBox)
        let turned = page.rotation % 180 != 0
        let size = turned ? CGSize(width: box.height, height: box.width) : box.size
        guard size.width > 1, size.height > 1 else { return nil }
        let scale = min(200.0 / 72.0, 3000 / max(size.width, size.height))
        let (width, height) = (Int(size.width * scale), Int(size.height * scale))
        guard let space = CGColorSpace(name: CGColorSpace.sRGB),
              let context = CGContext(
                  data: nil, width: width, height: height, bitsPerComponent: 8, bytesPerRow: 0, space: space,
                  bitmapInfo: CGImageAlphaInfo.noneSkipLast.rawValue)
        else { return nil }
        context.setFillColor(CGColor(red: 1, green: 1, blue: 1, alpha: 1))
        context.fill(CGRect(x: 0, y: 0, width: width, height: height))
        context.scaleBy(x: scale, y: scale)
        // PDFKit turns the page and moves its box to the origin itself.
        page.draw(with: .mediaBox, to: context)
        return context.makeImage()
    }
}
