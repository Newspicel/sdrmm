use sdrmm_wire::{
    MAX_NANOVNA_POINTS, NanoVnaAntenna, NanoVnaComplex, NanoVnaLibraryResponse, NanoVnaMeasurement,
    NanoVnaMeasurementInfo, NanoVnaPoint, SaveNanoVnaMeasurementRequest,
};

use super::*;

fn full_sweep() -> Vec<NanoVnaPoint> {
    (0..u64::from(MAX_NANOVNA_POINTS))
        .map(|index| {
            let wobble = index as f64 * 1.234_567_890_123e-5;
            NanoVnaPoint {
                frequency_hz: 1_000_000 + index * 1_000,
                s11: NanoVnaComplex {
                    re: 0.123_456_789_012_345_6 + wobble,
                    im: -0.234_567_890_123_456_7 + wobble,
                },
                s21: NanoVnaComplex {
                    re: 0.012_345_678_901_234_5,
                    im: -0.023_456_789_012_345_6,
                },
            }
        })
        .collect()
}

fn assert_same_points(got: &[NanoVnaPoint], sent: &[NanoVnaPoint]) {
    assert_eq!(got.len(), sent.len());
    for (got, sent) in got.iter().zip(sent) {
        assert_eq!(got.frequency_hz, sent.frequency_hz);
        for (a, b) in [
            (got.s11.re, sent.s11.re),
            (got.s11.im, sent.s11.im),
            (got.s21.re, sent.s21.re),
            (got.s21.im, sent.s21.im),
        ] {
            assert!((a - b).abs() < 1e-15, "{a} != {b}");
        }
    }
}

async fn create_antenna(app: &Router, name: &str) -> (StatusCode, Bytes) {
    request(
        app.clone(),
        "POST",
        "/api/nanovna/antennas",
        Some(&format!(r#"{{"name":"{name}"}}"#)),
    )
    .await
}

#[tokio::test]
async fn an_antenna_keeps_a_full_sweep_until_it_is_deleted() {
    let app = test_router();
    let (status, body) = create_antenna(&app, "40m dipole").await;
    assert_eq!(status, StatusCode::OK);
    let antenna: NanoVnaAntenna = serde_json::from_slice(&body).expect("json");
    let (status, _) = create_antenna(&app, "40M DIPOLE").await;
    assert_eq!(status, StatusCode::CONFLICT);

    let save = serde_json::to_string_pretty(&SaveNanoVnaMeasurementRequest {
        label: "Before trim".to_owned(),
        note: String::new(),
        instrument: Some("NanoVNA-H 4 1.2.46".to_owned()),
        points: full_sweep(),
    })
    .expect("serialize");
    assert!(save.len() > 2 * 1024 * 1024);
    let (status, body) = request(
        app.clone(),
        "POST",
        &format!("/api/nanovna/antennas/{}/measurements", antenna.id),
        Some(&save),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
    let info: NanoVnaMeasurementInfo = serde_json::from_slice(&body).expect("json");
    assert_eq!(info.point_count, MAX_NANOVNA_POINTS);

    let (status, body) = request(app.clone(), "GET", "/api/nanovna/library", None).await;
    assert_eq!(status, StatusCode::OK);
    let library: NanoVnaLibraryResponse = serde_json::from_slice(&body).expect("json");
    assert_eq!(library.antennas, std::slice::from_ref(&antenna));
    assert_eq!(library.measurements, std::slice::from_ref(&info));
    assert!(!String::from_utf8_lossy(&body).contains("frequency_hz"));

    let path = format!("/api/nanovna/measurements/{}", info.id);
    let (status, body) = request(
        app.clone(),
        "PATCH",
        &path,
        Some(r#"{"label":"After trim","note":"Shortened 4 cm"}"#),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let renamed: NanoVnaMeasurementInfo = serde_json::from_slice(&body).expect("json");
    assert_eq!(renamed.label, "After trim");

    let (status, body) = request(app.clone(), "GET", &path, None).await;
    assert_eq!(status, StatusCode::OK);
    let measurement: NanoVnaMeasurement = serde_json::from_slice(&body).expect("json");
    assert_eq!(measurement.info, renamed);
    assert_same_points(&measurement.points, &full_sweep());

    let (status, _) = request(
        app.clone(),
        "DELETE",
        &format!("/api/nanovna/antennas/{}", antenna.id),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _) = request(app, "GET", &path, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn unusable_requests_are_refused_with_the_right_status() {
    let app = test_router();
    let (status, _) = create_antenna(&app, "   ").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let (status, _) = request(
        app.clone(),
        "POST",
        "/api/nanovna/antennas/99/measurements",
        Some(
            r#"{"label":"Lost","points":[
                {"frequency_hz":1,"s11":{"re":0,"im":0},"s21":{"re":0,"im":0}},
                {"frequency_hz":2,"s11":{"re":0,"im":0},"s21":{"re":0,"im":0}}]}"#,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (_, body) = create_antenna(&app, "Vertical").await;
    let antenna: NanoVnaAntenna = serde_json::from_slice(&body).expect("json");
    let (status, _) = request(
        app.clone(),
        "POST",
        &format!("/api/nanovna/antennas/{}/measurements", antenna.id),
        Some(r#"{"label":"Empty","points":[]}"#),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let (status, _) = request(
        app,
        "PATCH",
        "/api/nanovna/measurements/99",
        Some(r#"{"label":"Nothing"}"#),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}
