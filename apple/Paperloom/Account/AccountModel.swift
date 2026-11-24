import AuthenticationServices
import CoreKit
import Foundation
import Observation
import Security
import SwiftUI

/// The signed-in Paperloom account, if any. Signing in opens the account
/// service's page in a private browser session; the tokens stay in the
/// keychain.
@Observable
final class AccountModel {
    private(set) var user: StoredSession.User?
    private(set) var isWorking = false
    var errorMessage: String?

    private let service: AccountService?

    /// False when this build has no client ID for the account service.
    var isAvailable: Bool { service != nil }

    init(bundle: Bundle = .main) {
        let clientID = bundle.object(forInfoDictionaryKey: "PaperloomAccountClientID") as? String ?? ""
        service = try? AccountService(clientId: clientID)
        user = AccountKeychain.load()?.user
    }

    @MainActor
    func signIn(with session: WebAuthenticationSession) async {
        guard let service, !isWorking else { return }
        isWorking = true
        defer { isWorking = false }
        do {
            let request = try await Task.detached { try service.beginSignIn() }.value
            guard let url = URL(string: request.url) else { return }
            let callback = try await session.authenticate(
                using: url,
                callback: .customScheme(accountCallbackScheme()),
                preferredBrowserSession: .ephemeral,
                additionalHeaderFields: [:])
            let signedIn = try await Task.detached {
                try service.finishSignIn(request: request, callbackUrl: callback.absoluteString)
            }.value
            let stored = StoredSession(signedIn)
            AccountKeychain.save(stored)
            user = stored.user
            errorMessage = nil
        } catch let error as ASWebAuthenticationSessionError where error.code == .canceledLogin {
            // The person closed the page.
        } catch {
            errorMessage = CoreKit.message(for: error)
        }
    }

    /// An access token for the sync service. The refresh token that comes
    /// back with it replaces the kept one (the service rotates them).
    func syncToken() async throws -> String {
        guard let service else { throw SyncNeedsSignIn() }
        guard var stored = AccountKeychain.load(), let refreshToken = stored.refreshToken else {
            throw SyncNeedsSignIn()
        }
        let tokens = try await Task.detached {
            try service.refresh(refreshToken: refreshToken, forSync: true)
        }.value
        if let rotated = tokens.refreshToken {
            stored.refreshToken = rotated
            AccountKeychain.save(stored)
        }
        return tokens.accessToken
    }

    /// Forgets the session on this device. Sign-in pages run in private
    /// browser sessions, so no browser cookie outlives it.
    @MainActor
    func signOut() {
        AccountKeychain.delete()
        user = nil
    }
}

/// Syncing needs a signed-in account.
struct SyncNeedsSignIn: LocalizedError {
    var errorDescription: String? {
        String(localized: "Sign in to sync your library across devices with Paperloom Cloud.")
    }
}

/// What the keychain keeps between launches.
struct StoredSession: Codable, Equatable {
    struct User: Codable, Equatable {
        var subject: String
        var displayName: String
        var email: String?
    }

    var user: User
    var refreshToken: String?
    var idToken: String?

    init(_ session: AccountSession) {
        user = User(subject: session.user.subject, displayName: session.user.displayName, email: session.user.email)
        refreshToken = session.tokens.refreshToken
        idToken = session.tokens.idToken
    }
}

/// One generic-password item holding the session as JSON.
enum AccountKeychain {
    private static let service = "io.felixbennett.paperloom.account"
    private static let account = "session"

    private static var query: [String: Any] {
        [kSecClass as String: kSecClassGenericPassword,
         kSecAttrService as String: service,
         kSecAttrAccount as String: account]
    }

    static func load() -> StoredSession? {
        var query = query
        query[kSecReturnData as String] = true
        query[kSecMatchLimit as String] = kSecMatchLimitOne
        var result: CFTypeRef?
        guard SecItemCopyMatching(query as CFDictionary, &result) == errSecSuccess, let data = result as? Data else {
            return nil
        }
        return try? JSONDecoder().decode(StoredSession.self, from: data)
    }

    static func save(_ session: StoredSession) {
        guard let data = try? JSONEncoder().encode(session) else { return }
        let attributes: [String: Any] = [
            kSecValueData as String: data,
            kSecAttrAccessible as String: kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly,
        ]
        if SecItemUpdate(query as CFDictionary, attributes as CFDictionary) == errSecItemNotFound {
            var item = query
            item.merge(attributes) { $1 }
            SecItemAdd(item as CFDictionary, nil)
        }
    }

    static func delete() {
        SecItemDelete(query as CFDictionary)
    }
}
