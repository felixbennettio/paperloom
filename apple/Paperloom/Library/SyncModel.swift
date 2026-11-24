import CoreKit
import Foundation
import Observation
import Security
import SwiftUI
#if os(iOS)
import UIKit
#endif

/// Where the library syncs.
enum SyncTarget: String, CaseIterable, Identifiable {
    case cloud, folder, webdav, s3, seafile

    var id: String { rawValue }

    var title: LocalizedStringKey {
        switch self {
        case .cloud: "Paperloom Cloud"
        case .folder: "A Folder"
        case .webdav: "WebDAV"
        case .s3: "S3 Storage"
        case .seafile: "Seafile"
        }
    }
}

/// The library syncs with Paperloom Cloud, through a folder other devices
/// reach, or through WebDAV, S3 or Seafile. This model keeps its status and
/// starts sync.
@MainActor
@Observable
final class SyncModel {
    static let defaultService = "https://sync.paperloom.routeforge.zone"

    var target: SyncTarget {
        didSet { UserDefaults.standard.set(target.rawValue, forKey: "syncTarget") }
    }
    /// The service's address, kept between launches.
    var service: String {
        didSet { UserDefaults.standard.set(service, forKey: "syncService") }
    }
    var webdavAddress: String {
        didSet { UserDefaults.standard.set(webdavAddress, forKey: "syncWebDAVAddress") }
    }
    var webdavUser: String {
        didSet { UserDefaults.standard.set(webdavUser, forKey: "syncWebDAVUser") }
    }
    /// Kept in the keychain.
    var webdavPassword: String {
        didSet { SecretKeychain.save(webdavPassword, SecretKeychain.webdav) }
    }
    var seafileServer: String {
        didSet { UserDefaults.standard.set(seafileServer, forKey: "syncSeafileServer") }
    }
    var seafileAccount: String {
        didSet { UserDefaults.standard.set(seafileAccount, forKey: "syncSeafileAccount") }
    }
    var seafileLibrary: String {
        didSet { UserDefaults.standard.set(seafileLibrary, forKey: "syncSeafileLibrary") }
    }
    var seafileFolder: String {
        didSet { UserDefaults.standard.set(seafileFolder, forKey: "syncSeafileFolder") }
    }
    /// The password or API token is kept in the keychain.
    var seafilePassword: String {
        didSet { SecretKeychain.save(seafilePassword, SecretKeychain.seafile) }
    }
    var s3Endpoint: String {
        didSet { UserDefaults.standard.set(s3Endpoint, forKey: "syncS3Endpoint") }
    }
    var s3Region: String {
        didSet { UserDefaults.standard.set(s3Region, forKey: "syncS3Region") }
    }
    var s3Bucket: String {
        didSet { UserDefaults.standard.set(s3Bucket, forKey: "syncS3Bucket") }
    }
    var s3Prefix: String {
        didSet { UserDefaults.standard.set(s3Prefix, forKey: "syncS3Prefix") }
    }
    var s3AccessKeyID: String {
        didSet { UserDefaults.standard.set(s3AccessKeyID, forKey: "syncS3AccessKeyID") }
    }
    /// Kept in the keychain.
    var s3Secret: String {
        didSet { SecretKeychain.save(s3Secret, SecretKeychain.s3) }
    }
    /// The chosen folder's path, for showing.
    private(set) var folderPath: String?
    private(set) var status: SyncStatus?
    private(set) var isSyncing = false
    /// Sync seals what it sends end to end (the passphrase is in the
    /// keychain).
    private(set) var sealed: Bool
    private(set) var isSealing = false
    private(set) var lastReport: SyncReport?
    var errorMessage: String?

    init() {
        let defaults = UserDefaults.standard
        target = defaults.string(forKey: "syncTarget").flatMap(SyncTarget.init(rawValue:)) ?? .cloud
        service = defaults.string(forKey: "syncService") ?? Self.defaultService
        webdavAddress = defaults.string(forKey: "syncWebDAVAddress") ?? ""
        webdavUser = defaults.string(forKey: "syncWebDAVUser") ?? ""
        webdavPassword = SecretKeychain.load(SecretKeychain.webdav) ?? ""
        seafileServer = defaults.string(forKey: "syncSeafileServer") ?? ""
        seafileAccount = defaults.string(forKey: "syncSeafileAccount") ?? ""
        seafileLibrary = defaults.string(forKey: "syncSeafileLibrary") ?? ""
        seafileFolder = defaults.string(forKey: "syncSeafileFolder") ?? ""
        seafilePassword = SecretKeychain.load(SecretKeychain.seafile) ?? ""
        s3Endpoint = defaults.string(forKey: "syncS3Endpoint") ?? ""
        s3Region = defaults.string(forKey: "syncS3Region") ?? ""
        s3Bucket = defaults.string(forKey: "syncS3Bucket") ?? ""
        s3Prefix = defaults.string(forKey: "syncS3Prefix") ?? ""
        s3AccessKeyID = defaults.string(forKey: "syncS3AccessKeyID") ?? ""
        s3Secret = SecretKeychain.load(SecretKeychain.s3) ?? ""
        sealed = SecretKeychain.load(SecretKeychain.syncPassphrase) != nil
        folderPath = (try? Self.resolveFolder())?.path(percentEncoded: false)
    }

    /// Seals sync end to end with a passphrase (kept in the keychain), or,
    /// empty, stops sealing. Making the key takes a moment.
    func setPassphrase(_ passphrase: String, library: LibraryModel) async {
        guard let core = library.core, !isSealing else { return }
        isSealing = true
        defer { isSealing = false }
        let wanted: String? = passphrase.isEmpty ? nil : passphrase
        do {
            try await Task.detached { try core.setSyncPassphrase(passphrase: wanted) }.value
            SecretKeychain.save(passphrase, SecretKeychain.syncPassphrase)
            sealed = wanted != nil
            errorMessage = nil
        } catch {
            errorMessage = CoreKit.message(for: error)
        }
    }

    /// Whether Sync Now can work with what is filled in.
    func isReady(signedIn: Bool) -> Bool {
        switch target {
        case .cloud: signedIn && !service.isEmpty
        case .folder: folderPath != nil
        case .webdav: !webdavAddress.isEmpty && !webdavUser.isEmpty && !webdavPassword.isEmpty
        case .s3: !s3Endpoint.isEmpty && !s3Bucket.isEmpty && !s3AccessKeyID.isEmpty && !s3Secret.isEmpty
        case .seafile: !seafileServer.isEmpty && !seafileAccount.isEmpty && !seafileLibrary.isEmpty && !seafilePassword.isEmpty
        }
    }

    /// Remembers a folder chosen in the file picker, across launches.
    func chooseFolder(_ url: URL) {
        let scoped = url.startAccessingSecurityScopedResource()
        defer { if scoped { url.stopAccessingSecurityScopedResource() } }
        do {
            #if os(macOS)
            let bookmark = try url.bookmarkData(
                options: .withSecurityScope, includingResourceValuesForKeys: nil, relativeTo: nil)
            #else
            let bookmark = try url.bookmarkData()
            #endif
            UserDefaults.standard.set(bookmark, forKey: "syncFolderBookmark")
            folderPath = url.path(percentEncoded: false)
            errorMessage = nil
        } catch {
            errorMessage = error.localizedDescription
        }
    }

    private static func resolveFolder() throws -> URL? {
        guard let bookmark = UserDefaults.standard.data(forKey: "syncFolderBookmark") else { return nil }
        var stale = false
        #if os(macOS)
        return try URL(resolvingBookmarkData: bookmark, options: .withSecurityScope, relativeTo: nil, bookmarkDataIsStale: &stale)
        #else
        return try URL(resolvingBookmarkData: bookmark, options: [], relativeTo: nil, bookmarkDataIsStale: &stale)
        #endif
    }

    func refresh(library: LibraryModel) {
        guard let core = library.core else { return }
        status = try? core.syncStatus()
    }

    /// One sync of the library and the vault's notes.
    func syncNow(library: LibraryModel, vault: VaultModel, account: AccountModel) async {
        guard let core = library.core, !isSyncing else { return }
        let notes = vault.core
        isSyncing = true
        errorMessage = nil
        do {
            // The key is made the first time it is needed after starting.
            let passphrase = SecretKeychain.load(SecretKeychain.syncPassphrase)
            if (passphrase != nil) != core.syncSealed() {
                try await Task.detached { try core.setSyncPassphrase(passphrase: passphrase) }.value
            }
            let report: SyncReport
            switch target {
            case .cloud:
                let address = service.trimmingCharacters(in: .whitespacesAndNewlines)
                let device = Self.deviceName
                let platform = Self.platform
                let token = try await account.syncToken()
                report = try await Task.detached {
                    try core.syncWithCloud(
                        service: address, accessToken: token, deviceName: device, platform: platform, vault: notes)
                }.value
            case .folder:
                guard let folder = try Self.resolveFolder() else { throw SyncNeedsFolder() }
                report = try await Task.detached {
                    let scoped = folder.startAccessingSecurityScopedResource()
                    defer { if scoped { folder.stopAccessingSecurityScopedResource() } }
                    return try core.syncWithFolder(folder: folder.path(percentEncoded: false), vault: notes)
                }.value
            case .webdav:
                let address = webdavAddress.trimmingCharacters(in: .whitespacesAndNewlines)
                let user = webdavUser
                let password = webdavPassword
                report = try await Task.detached {
                    try core.syncWithWebdav(address: address, username: user, password: password, vault: notes)
                }.value
            case .s3:
                let trim = { (text: String) in text.trimmingCharacters(in: .whitespacesAndNewlines) }
                let place = S3Place(
                    endpoint: trim(s3Endpoint), region: trim(s3Region), bucket: trim(s3Bucket),
                    prefix: trim(s3Prefix), accessKeyId: trim(s3AccessKeyID))
                let secret = s3Secret
                report = try await Task.detached {
                    try core.syncWithS3(place: place, secretKey: secret, vault: notes)
                }.value
            case .seafile:
                let trim = { (text: String) in text.trimmingCharacters(in: .whitespacesAndNewlines) }
                let place = SeafilePlace(
                    server: trim(seafileServer), account: trim(seafileAccount),
                    library: trim(seafileLibrary), folder: trim(seafileFolder))
                let password = seafilePassword
                report = try await Task.detached {
                    try core.syncWithSeafile(place: place, password: password, vault: notes)
                }.value
            }
            lastReport = report
            if report.changed > 0 || report.downloaded > 0 {
                library.reload()
            }
            if report.notes > 0 {
                vault.reloadAfterSync()
            }
        } catch {
            errorMessage = CoreKit.message(for: error)
        }
        isSyncing = false
        refresh(library: library)
    }

    /// The summary under the Sync button.
    var summary: String {
        var parts: [String] = []
        if let last = status?.lastSynced {
            let date = Date(timeIntervalSince1970: TimeInterval(last) / 1000)
            let time = date.formatted(date: .abbreviated, time: .shortened)
            parts.append(String(localized: "Last synced \(time)"))
        } else {
            parts.append(String(localized: "Not synced yet"))
        }
        if let waiting = status?.waiting, waiting > 0 {
            parts.append(String(localized: "Changes waiting: \(String(waiting))"))
        }
        return parts.joined(separator: " · ")
    }

    private static var deviceName: String {
        #if os(macOS)
        Host.current().localizedName ?? "Mac"
        #else
        UIDevice.current.name
        #endif
    }

    private static var platform: String {
        #if os(macOS)
        "macos"
        #else
        "ios"
        #endif
    }
}

/// Syncing through a folder needs one chosen.
struct SyncNeedsFolder: LocalizedError {
    var errorDescription: String? { String(localized: "Choose a folder to sync through.") }
}

/// A secret kept in the keychain, one generic-password item per service.
enum SecretKeychain {
    static let webdav = "io.felixbennett.paperloom.webdav"
    static let seafile = "io.felixbennett.paperloom.seafile"
    static let ai = "io.felixbennett.paperloom.ai"
    static let s3 = "io.felixbennett.paperloom.s3"
    static let syncPassphrase = "io.felixbennett.paperloom.sync-passphrase"

    private static func query(_ service: String) -> [String: Any] {
        [kSecClass as String: kSecClassGenericPassword,
         kSecAttrService as String: service,
         kSecAttrAccount as String: "secret"]
    }

    static func load(_ service: String) -> String? {
        var query = query(service)
        query[kSecReturnData as String] = true
        query[kSecMatchLimit as String] = kSecMatchLimitOne
        var result: CFTypeRef?
        guard SecItemCopyMatching(query as CFDictionary, &result) == errSecSuccess, let data = result as? Data else {
            return nil
        }
        return String(data: data, encoding: .utf8)
    }

    /// Keeps a secret; an empty one is forgotten.
    static func save(_ secret: String, _ service: String) {
        let query = query(service)
        guard !secret.isEmpty else {
            SecItemDelete(query as CFDictionary)
            return
        }
        let attributes: [String: Any] = [
            kSecValueData as String: Data(secret.utf8),
            kSecAttrAccessible as String: kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly,
        ]
        if SecItemUpdate(query as CFDictionary, attributes as CFDictionary) == errSecItemNotFound {
            var item = query
            item.merge(attributes) { $1 }
            SecItemAdd(item as CFDictionary, nil)
        }
    }
}
