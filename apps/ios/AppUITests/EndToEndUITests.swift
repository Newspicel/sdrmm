import XCTest

@MainActor
final class EndToEndUITests: XCTestCase {
    private let patience: TimeInterval = 30

    func testPairAndHuntAgainstServer() async throws {
        let server = try PairingServer(environment: ProcessInfo.processInfo.environment)
        allowLocation()
        let app = XCUIApplication()
        app.launchEnvironment["SDRMM_E2E"] = "1"
        app.launchEnvironment["SDRMM_UITEST"] = "1"
        app.launch()
        XCTAssertTrue(app.navigationBars["Pair"].waitForExistence(timeout: patience))
        try await app.open(server.freshLink())
        let trust = app.buttons["pair.trust"]
        XCTAssertTrue(trust.waitForExistence(timeout: patience))
        trust.tap()
        let hunt = app.buttons["missions.row.hunt"]
        let refusal = app.staticTexts["pair.error"]
        let paired = hunt.waitForExistence(timeout: patience)
        XCTAssertTrue(paired, refusal.exists ? refusal.label : "No mission list")
        XCTAssertTrue(hunt.label.contains("Fox"), hunt.label)
        hunt.tap()
        let run = app.buttons["hunt.run"]
        XCTAssertTrue(run.waitForLabel("Start", timeout: patience), run.label)
        run.tap()
        XCTAssertTrue(run.waitForLabel("Stop", timeout: patience), run.label)
        let trend = app.staticTexts["hunt.trend"]
        XCTAssertTrue(trend.waitForExistence(timeout: patience))
        XCTAssertTrue(trend.waitForLabel(timeout: 10) { $0 != "Listening" }, trend.label)
        run.tap()
        XCTAssertTrue(run.waitForLabel("Start", timeout: patience), run.label)
    }

    private func allowLocation() {
        addUIInterruptionMonitor(withDescription: "Location") { alert in
            for label in ["Allow While Using App", "Allow Once", "Allow"] {
                let button = alert.buttons[label]
                if button.exists {
                    button.tap()
                    return true
                }
            }
            return false
        }
    }
}
