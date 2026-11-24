import Foundation
import XCTest

struct PairingServer {
    private let base: URL
    private let token: String
    private let phone: String

    init(environment: [String: String]) throws {
        guard let base = environment["SDRMM_E2E_SERVER"].flatMap(URL.init(string:)),
            let token = environment["SDRMM_E2E_TOKEN"],
            let phone = environment["SDRMM_E2E_PHONE"]
        else {
            throw XCTSkip("Needs cargo xtask ios e2e")
        }
        self.base = base
        self.token = token
        self.phone = phone
    }

    func freshLink() async throws -> URL {
        var request = URLRequest(url: base.appending(path: "api/phones/offers"))
        request.httpMethod = "POST"
        request.setValue("Bearer \(token)", forHTTPHeaderField: "Authorization")
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        request.httpBody = Data("{}".utf8)
        let session = URLSession(configuration: .ephemeral, delegate: TrustLoopback(), delegateQueue: nil)
        defer { session.finishTasksAndInvalidate() }
        let (body, response) = try await session.data(for: request)
        let status = (response as? HTTPURLResponse)?.statusCode ?? 0
        guard status == 201 else {
            throw OfferRefused(status: status, body: String(decoding: body, as: UTF8.self))
        }
        return try onLoopback(JSONDecoder().decode(Offer.self, from: body).uri)
    }

    private func onLoopback(_ uri: String) throws -> URL {
        var parts = try XCTUnwrap(URLComponents(string: uri), uri)
        let rest = (parts.queryItems ?? []).filter { $0.name != "h" }
        parts.queryItems = [URLQueryItem(name: "h", value: phone)] + rest
        return try XCTUnwrap(parts.url, uri)
    }
}

private struct Offer: Decodable {
    let uri: String
}

private struct OfferRefused: LocalizedError {
    let status: Int
    let body: String

    var errorDescription: String? { "POST /api/phones/offers answered \(status): \(body)" }
}

private final class TrustLoopback: NSObject, URLSessionDelegate, Sendable {
    func urlSession(
        _ session: URLSession,
        didReceive challenge: URLAuthenticationChallenge
    ) async -> (URLSession.AuthChallengeDisposition, URLCredential?) {
        guard challenge.protectionSpace.host == "127.0.0.1",
            let trust = challenge.protectionSpace.serverTrust
        else {
            return (.performDefaultHandling, nil)
        }
        return (.useCredential, URLCredential(trust: trust))
    }
}
