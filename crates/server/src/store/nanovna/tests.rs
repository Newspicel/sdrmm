use super::*;

fn store() -> Store {
    Store::open(None).expect("in-memory store")
}

fn point(frequency_hz: u64, re: f64, im: f64) -> NanoVnaPoint {
    NanoVnaPoint {
        frequency_hz,
        s11: NanoVnaComplex { re, im },
        s21: NanoVnaComplex { re: 0.0, im: 0.0 },
    }
}

fn sweep() -> Vec<NanoVnaPoint> {
    vec![
        point(7_000_000, 0.5, 0.0),
        point(7_100_000, 0.0, 0.2),
        point(7_200_000, -0.6, 0.1),
    ]
}

fn antenna(store: &Store, name: &str) -> NanoVnaAntenna {
    store
        .create_nanovna_antenna(&NanoVnaAntennaRequest {
            name: name.to_owned(),
            notes: String::new(),
        })
        .expect("antenna")
}

fn save(store: &Store, antenna_id: i64, label: &str) -> NanoVnaMeasurementInfo {
    store
        .save_nanovna_measurement(
            antenna_id,
            &SaveNanoVnaMeasurementRequest {
                label: label.to_owned(),
                note: String::new(),
                instrument: Some("NanoVNA-H 4 1.2.46".to_owned()),
                points: sweep(),
            },
        )
        .expect("measurement")
}

#[test]
fn antennas_list_by_name_and_names_are_unique_ignoring_case() {
    let store = store();
    antenna(&store, "  Vertical ");
    let dipole = antenna(&store, "40m dipole");
    let names: Vec<_> = store
        .list_nanovna_antennas()
        .expect("list")
        .into_iter()
        .map(|antenna| antenna.name)
        .collect();
    assert_eq!(names, ["40m dipole", "Vertical"]);

    let duplicate = store.create_nanovna_antenna(&NanoVnaAntennaRequest {
        name: "VERTICAL".to_owned(),
        notes: String::new(),
    });
    assert!(matches!(duplicate, Err(StoreError::NanoVnaNameTaken)));

    let renamed = store
        .update_nanovna_antenna(
            dipole.id,
            &NanoVnaAntennaRequest {
                name: "40m inverted V".to_owned(),
                notes: "Fed with RG-58".to_owned(),
            },
        )
        .expect("rename");
    assert_eq!(renamed.notes, "Fed with RG-58");
    assert!(matches!(
        store.update_nanovna_antenna(
            dipole.id,
            &NanoVnaAntennaRequest {
                name: "vertical".to_owned(),
                notes: String::new(),
            }
        ),
        Err(StoreError::NanoVnaNameTaken)
    ));
}

#[test]
fn a_saved_measurement_keeps_its_points_and_best_match() {
    let store = store();
    let dipole = antenna(&store, "Dipole");
    let info = save(&store, dipole.id, "  First ");
    assert_eq!(info.label, "First");
    assert_eq!(info.antenna_id, dipole.id);
    assert_eq!(
        (info.start_hz, info.stop_hz, info.point_count),
        (7_000_000, 7_200_000, 3)
    );
    assert_eq!(info.best_hz, Some(7_100_000));
    let best = info.best_vswr.expect("best vswr");
    assert!((best - 1.2 / 0.8).abs() < 1e-12);
    assert_eq!(info.instrument.as_deref(), Some("NanoVNA-H 4 1.2.46"));

    let measurement = store.nanovna_measurement(info.id).expect("get");
    assert_eq!(measurement.info, info);
    assert_eq!(measurement.points, sweep());
}

#[test]
fn measurements_list_newest_first_and_can_be_relabelled() {
    let store = store();
    let dipole = antenna(&store, "Dipole");
    let first = save(&store, dipole.id, "Before");
    let second = save(&store, dipole.id, "After");
    let ids: Vec<_> = store
        .list_nanovna_measurements()
        .expect("list")
        .into_iter()
        .map(|info| info.id)
        .collect();
    assert_eq!(ids, [second.id, first.id]);

    let updated = store
        .update_nanovna_measurement(
            first.id,
            &UpdateNanoVnaMeasurementRequest {
                label: "Before trim".to_owned(),
                note: "Wet weather".to_owned(),
            },
        )
        .expect("update");
    assert_eq!(
        (updated.label.as_str(), updated.note.as_str()),
        ("Before trim", "Wet weather")
    );

    store.delete_nanovna_measurement(first.id).expect("delete");
    assert!(matches!(
        store.nanovna_measurement(first.id),
        Err(StoreError::NanoVnaMeasurementNotFound(_))
    ));
    assert!(matches!(
        store.delete_nanovna_measurement(first.id),
        Err(StoreError::NanoVnaMeasurementNotFound(_))
    ));
}

#[test]
fn deleting_an_antenna_takes_its_measurements_along() {
    let store = store();
    let dipole = antenna(&store, "Dipole");
    let vertical = antenna(&store, "Vertical");
    let gone = save(&store, dipole.id, "Gone");
    let kept = save(&store, vertical.id, "Kept");

    store.delete_nanovna_antenna(dipole.id).expect("delete");
    let left: Vec<_> = store
        .list_nanovna_measurements()
        .expect("list")
        .into_iter()
        .map(|info| info.id)
        .collect();
    assert_eq!(left, [kept.id]);
    assert!(store.nanovna_measurement(gone.id).is_err());
    assert!(matches!(
        store.delete_nanovna_antenna(dipole.id),
        Err(StoreError::NanoVnaAntennaNotFound(_))
    ));
    assert!(matches!(
        store.save_nanovna_measurement(
            dipole.id,
            &SaveNanoVnaMeasurementRequest {
                label: "Late".to_owned(),
                note: String::new(),
                instrument: None,
                points: sweep(),
            }
        ),
        Err(StoreError::NanoVnaAntennaNotFound(_))
    ));
}

#[test]
fn unusable_fields_and_points_are_refused() {
    let store = store();
    let dipole = antenna(&store, "Dipole");
    let request = |label: &str, points: Vec<NanoVnaPoint>| SaveNanoVnaMeasurementRequest {
        label: label.to_owned(),
        note: String::new(),
        instrument: None,
        points,
    };
    let refused = |request: SaveNanoVnaMeasurementRequest| match store
        .save_nanovna_measurement(dipole.id, &request)
    {
        Err(StoreError::NanoVnaField(field)) => field,
        other => panic!("expected a refused field, got {other:?}"),
    };
    assert_eq!(refused(request("   ", sweep())), "label");
    assert_eq!(
        refused(request(&"x".repeat(MAX_NANOVNA_NAME_LEN + 1), sweep())),
        "label"
    );
    assert_eq!(
        refused(request("One", vec![point(7_000_000, 0.1, 0.0)])),
        "point count"
    );
    assert_eq!(
        refused(request(
            "NaN",
            vec![point(1, 0.1, 0.0), point(2, f64::NAN, 0.0)]
        )),
        "point values"
    );
    assert_eq!(
        refused(request(
            "Order",
            vec![point(2, 0.1, 0.0), point(2, 0.1, 0.0)]
        )),
        "frequency order"
    );
    let mut long_note = request("Note", sweep());
    long_note.note = "x".repeat(MAX_NANOVNA_NOTE_LEN + 1);
    assert_eq!(refused(long_note), "note");
    assert!(matches!(
        store.create_nanovna_antenna(&NanoVnaAntennaRequest {
            name: String::new(),
            notes: String::new(),
        }),
        Err(StoreError::NanoVnaField("name"))
    ));
}

#[test]
fn a_sweep_with_no_point_inside_the_unit_circle_has_no_best_match() {
    let store = store();
    let dipole = antenna(&store, "Dipole");
    let info = store
        .save_nanovna_measurement(
            dipole.id,
            &SaveNanoVnaMeasurementRequest {
                label: "Open".to_owned(),
                note: String::new(),
                instrument: Some("  ".to_owned()),
                points: vec![point(1_000_000, 1.0, 0.0), point(2_000_000, 0.0, -1.0)],
            },
        )
        .expect("save");
    assert_eq!((info.best_hz, info.best_vswr), (None, None));
    assert_eq!(info.instrument, None);
}
