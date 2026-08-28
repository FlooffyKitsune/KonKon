use konkon_core::script::placeholder::{denormalize, normalize, ControlCodeSpec, ControlPattern};

fn specs() -> Vec<ControlCodeSpec> {
    vec![
        ControlCodeSpec {
            name: "newline",
            pattern: ControlPattern::Literal("\\n"),
        },
        ControlCodeSpec {
            name: "ruby",
            pattern: ControlPattern::Regex(r#"\[ruby text="[^"]*"\]"#),
        },
    ]
}

#[test]
fn normalize_replaces_codes_in_first_seen_order() {
    let raw = r#"芳乃は[ruby text="ほうの"]と\n名乗った\n"#;
    let (norm, map) = normalize(raw, &specs());
    assert_eq!(norm, "芳乃は{0}と{1}名乗った{1}");
    assert_eq!(map.get("0"), Some(r#"[ruby text="ほうの"]"#));
    assert_eq!(map.get("1"), Some(r"\n"));
}

#[test]
fn denormalize_is_inverse_of_normalize() {
    let raw = r#"待って[ruby text="ま"]……\n"#;
    let (norm, map) = normalize(raw, &specs());
    let back = denormalize(&norm, &map).unwrap();
    assert_eq!(back, raw);
}

#[test]
fn denormalize_rejects_unknown_placeholder() {
    let raw = r"hello\n";
    let (_norm, map) = normalize(raw, &specs());
    let err = denormalize("hello{0}{5}", &map).unwrap_err();
    assert!(err.to_string().contains("{5}"));
}

#[test]
fn translated_text_may_reorder_placeholders() {
    // EN word order differs; both placeholders still present exactly once each pattern class
    let raw = r#"[ruby text="A"]\n"#;
    let (_n, map) = normalize(raw, &specs());
    let back = denormalize("{1}{0}", &map).unwrap();
    assert_eq!(back, r#"\n[ruby text="A"]"#);
}

#[test]
fn denormalize_rejects_dropped_placeholder() {
    let raw = r#"[ruby text="A"]\n"#;
    let (_n, map) = normalize(raw, &specs()); // 2 keys: {0}, {1}
    let err = denormalize("{0}", &map).unwrap_err(); // {1} dropped
    assert!(err.to_string().contains("{1}"));
}

// --- Focused tests added for overlap resolution (controller note 1) ---

fn overlap_specs() -> Vec<ControlCodeSpec> {
    vec![
        // Longer, outer pattern.
        ControlCodeSpec {
            name: "ruby",
            pattern: ControlPattern::Regex(r#"\[ruby text="[^"]*"\]"#),
        },
        // Shorter literal that also occurs *inside* every ruby tag.
        ControlCodeSpec {
            name: "attr",
            pattern: ControlPattern::Literal("text="),
        },
    ]
}

#[test]
fn normalize_drops_matches_overlapping_an_earlier_kept_match() {
    let raw = r#"x[ruby text="A"]y"#;
    let (norm, map) = normalize(raw, &overlap_specs());
    // The inner `text=` match overlaps the ruby match that starts earlier, so it is dropped.
    assert_eq!(norm, "x{0}y");
    assert_eq!(map.get("0"), Some(r#"[ruby text="A"]"#));
    assert_eq!(map.get("1"), None);
    assert_eq!(denormalize(&norm, &map).unwrap(), raw);
}

#[test]
fn normalize_prefers_longer_match_on_equal_start() {
    let specs = vec![
        ControlCodeSpec {
            name: "short",
            pattern: ControlPattern::Literal("AB"),
        },
        ControlCodeSpec {
            name: "long",
            pattern: ControlPattern::Regex("ABC"),
        },
    ];
    let (norm, map) = normalize("xABCy", &specs);
    assert_eq!(norm, "x{0}y");
    assert_eq!(map.get("0"), Some("ABC"));
}

#[test]
fn normalize_reuses_index_for_repeated_identical_code() {
    let specs = vec![ControlCodeSpec {
        name: "newline",
        pattern: ControlPattern::Literal("\\n"),
    }];
    let (norm, map) = normalize(r"a\nb\nc\n", &specs);
    assert_eq!(norm, "a{0}b{0}c{0}");
    assert_eq!(map.len(), 1);
    assert_eq!(map.get("0"), Some(r"\n"));
}

#[test]
fn placeholder_map_json_round_trips() {
    let (_n, map) = normalize(r#"[ruby text="x"]\n"#, &specs());
    let json = map.to_json();
    let restored = konkon_core::script::placeholder::PlaceholderMap::from_json(&json).unwrap();
    assert_eq!(restored.get("0"), map.get("0"));
    assert_eq!(restored.get("1"), map.get("1"));
}
