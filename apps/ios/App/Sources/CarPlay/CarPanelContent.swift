import SdrmmCore

nonisolated struct CarPanelContent: Equatable, Sendable {
    let bearing: String
    let bearingDetail: String
    let confidence: String?
    let guidance: String
    let guidanceDetail: String
    let canNavigate: Bool
    let canCalibrate: Bool
    let canClear: Bool
    let targetDistanceM: Double?

    static func make(
        df: DfView?,
        pose: PoseView?,
        here: LatLon?,
        units: UnitSystem,
        controls: [MissionControl] = []
    ) -> CarPanelContent {
        let rose = RoseState.make(view: df, pose: pose)
        let confidence = DfText.confidence(df)
        let detail: String
        if let confidence {
            detail = "Bearing \(confidence)"
        } else {
            detail = df.flatMap { DfText.state($0.state) } ?? "Waiting"
        }
        let guidance = df?.guidance
        return CarPanelContent(
            bearing: rose.bearingDeg.map(AngleText.degrees) ?? "-",
            bearingDetail: detail,
            confidence: confidence,
            guidance: guidance.map { AngleText.degrees($0.headingTrueDeg) } ?? "-",
            guidanceDetail: guidanceDetail(guidance, units: units),
            canNavigate: df?.target != nil,
            canCalibrate: controls.contains(.calibrate),
            canClear: controls.contains(.clearFusion),
            targetDistanceM: zip(here, df?.target).map { geoDistanceM(from: $0, to: $1.at) }
        )
    }

    static func guidanceDetail(_ guidance: GuidanceView?, units: UnitSystem) -> String {
        guard let guidance else {
            return "No guidance"
        }
        let distance = DistanceText.short(guidance.distanceM, units)
        switch guidance.kind {
        case .probe: return "Cross \(distance)"
        case .estimate: return "Approach \(distance)"
        }
    }

    private static func zip<A, B>(_ first: A?, _ second: B?) -> (A, B)? {
        guard let first, let second else {
            return nil
        }
        return (first, second)
    }
}
