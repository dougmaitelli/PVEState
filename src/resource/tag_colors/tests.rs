use super::*;
use crate::reconcile::Plan;
use serde_json::json;

struct CapturedOptions(Value);

impl PveClient for CapturedOptions {
    fn endpoint(&self) -> &str {
        "https://pve.test:8006"
    }
    fn get(&self, path: &str) -> Result<Value> {
        assert_eq!(path, "/cluster/options");
        Ok(self.0.clone())
    }
    fn put(&self, _: &str, _: &BTreeMap<String, String>) -> Result<()> {
        panic!("capture is read-only")
    }
    fn post(&self, _: &str, _: &BTreeMap<String, String>) -> Result<()> {
        panic!("capture is read-only")
    }
    fn delete(&self, _: &str, _: &BTreeMap<String, String>) -> Result<()> {
        panic!("capture is read-only")
    }
}

fn build(local: &LocalState, pve: &CapturedOptions) -> Plan {
    let mut builder = PlanBuilder::new("capture", pve.endpoint(), "https://pbs.test:8007");
    plan(local, pve, &mut builder).unwrap();
    let plan = builder.finish().unwrap();
    // Exercise the same serialization and authorization as a saved apply plan.
    Plan::from_slice(&serde_json::to_vec(&plan).unwrap())
        .unwrap()
        .verify()
        .unwrap();
    plan
}

#[test]
fn colors_decode_from_api_objects_and_property_strings_semantically() {
    let object = TagStyle::from_api(Some(&json!({
        "color-map": "web:AABBCC:FFFFFF;db:001122", "shape": "full", "case-sensitive": false,
        "future-option": "preserved"
    })))
    .unwrap();
    let text = TagStyle::from_api(Some(&json!(
        "shape=full,color-map=db:001122;web:aabbcc:ffffff,case-sensitive=0,future-option=preserved"
    )))
    .unwrap();
    assert_eq!(object.colors, text.colors);
    assert_eq!(object.render(), text.render());
    assert_eq!(
        text.render().unwrap(),
        "case-sensitive=0,color-map=db:001122;web:aabbcc:ffffff,future-option=preserved,shape=full"
    );
}

#[test]
fn rejects_invalid_names_colors_and_malformed_captured_styles() {
    for name in ["", "-bad", "white space", "tag;other", "tag:color"] {
        assert!(
            serde_json::from_value::<TagColors>(json!({name: {"background": "aabbcc"}})).is_err()
        );
    }
    for color in ["", "fff", "#aabbcc", "aabbccdd", "gg0000", "00000\n"] {
        assert!(
            serde_json::from_value::<TagColors>(json!({"tag": {"background": color}})).is_err()
        );
        assert!(
            serde_json::from_value::<TagColors>(
                json!({"tag": {"background": "000000", "text": color}})
            )
            .is_err()
        );
    }
    for value in [
        json!("color-map=tag:000000;tag:ffffff"),
        json!("color-map=tag:000000:ffffff:extra"),
        json!("color-map=tag:000000;"),
        json!("color-map=tag:000000,color-map=other:ffffff"),
        json!("broken"),
        json!({"color-map": []}),
        json!(42),
    ] {
        assert!(
            TagStyle::from_api(Some(&value)).is_err(),
            "accepted {value}"
        );
    }
}

#[test]
fn updates_adds_and_removes_colors_preserving_other_style_properties() {
    let temp = tempfile::tempdir().unwrap();
    crate::config::scaffold::initialize(temp.path()).unwrap();
    let mut local = crate::config::open(temp.path()).unwrap();
    local.cluster.tag_colors = Some(
        serde_json::from_value(json!({
            "web": {"background": "AABBCC", "text": "FFFFFF"},
            "new": {"background": "123456"}
        }))
        .unwrap(),
    );
    let pve = CapturedOptions(json!({"tag-style": {
        "color-map": "web:000000;removed:ff0000", "shape": "full", "ordering": "config",
        "case-sensitive": 1, "future-option": "preserved"
    }}));
    let plan = build(&local, &pve);
    assert_eq!(plan.operations.len(), 1);
    let Operation::ApiMutation {
        changes,
        before_values,
        ..
    } = &plan.operations[0]
    else {
        unreachable!()
    };
    assert_eq!(
        changes["tag-style"],
        "case-sensitive=1,color-map=new:123456;web:aabbcc:ffffff,future-option=preserved,ordering=config,shape=full"
    );
    assert_eq!(
        before_values["tag-style"].as_deref(),
        Some(
            "case-sensitive=1,color-map=removed:ff0000;web:000000,future-option=preserved,ordering=config,shape=full"
        )
    );
    let applied = CapturedOptions(json!({"tag-style": changes["tag-style"]}));
    assert!(build(&local, &applied).operations.is_empty());
}

#[test]
fn omitted_empty_and_new_maps_have_distinct_ownership() {
    let temp = tempfile::tempdir().unwrap();
    crate::config::scaffold::initialize(temp.path()).unwrap();
    let mut local = crate::config::open(temp.path()).unwrap();
    let pve = CapturedOptions(json!({"tag-style": "color-map=web:000000"}));
    assert!(build(&local, &pve).operations.is_empty());
    assert_eq!(
        candidates(&local, &pve).unwrap()[0].id,
        "cluster:tag_colors"
    );

    local.cluster.tag_colors = Some(BTreeMap::new());
    let plan = build(&local, &pve);
    let Operation::ApiMutation { changes, .. } = &plan.operations[0] else {
        unreachable!()
    };
    assert_eq!(
        changes,
        &BTreeMap::from([("delete".into(), "tag-style".into())])
    );
    assert!(
        build(&local, &CapturedOptions(json!({})))
            .operations
            .is_empty()
    );

    let with_shape = CapturedOptions(json!({"tag-style": "color-map=web:000000,shape=full"}));
    let plan = build(&local, &with_shape);
    let Operation::ApiMutation { changes, .. } = &plan.operations[0] else {
        unreachable!()
    };
    assert_eq!(
        changes,
        &BTreeMap::from([("tag-style".into(), "shape=full".into())])
    );

    local.cluster.tag_colors =
        Some(serde_json::from_value(json!({"web": {"background": "abcdef"}})).unwrap());
    let plan = build(&local, &CapturedOptions(json!({})));
    let Operation::ApiMutation {
        changes,
        before_values,
        ..
    } = &plan.operations[0]
    else {
        unreachable!()
    };
    assert_eq!(before_values["tag-style"], None);
    assert_eq!(changes["tag-style"], "color-map=web:abcdef");
}

#[test]
fn adopting_colors_then_reloading_converges_and_preserves_other_configuration() {
    let temp = tempfile::tempdir().unwrap();
    crate::config::scaffold::initialize(temp.path()).unwrap();
    let mut local = crate::config::open(temp.path()).unwrap();
    let pve =
        CapturedOptions(json!({"tag-style": {"color-map": "web:ABCDEF:000000", "shape": "dense"}}));
    for captured in [&pve, &CapturedOptions(json!({}))] {
        let candidates = candidates(&local, captured).unwrap();
        assert_eq!(candidates.len(), 1);
        let path = local.root().join("config/cluster.yml");
        let original = std::fs::read_to_string(&path).unwrap();
        let patches = candidates[0]
            .patches()
            .unwrap()
            .iter()
            .cloned()
            .map(LocalPatch::into_yaml_patch)
            .collect::<Vec<_>>();
        let adopted = crate::utility::yaml_patch::apply_patches(&original, &patches).unwrap();
        let mut before: serde_yaml::Value = serde_yaml::from_str(&original).unwrap();
        let mut after: serde_yaml::Value = serde_yaml::from_str(&adopted).unwrap();
        before
            .as_mapping_mut()
            .unwrap()
            .remove(serde_yaml::Value::String("tag_colors".into()));
        after
            .as_mapping_mut()
            .unwrap()
            .remove(serde_yaml::Value::String("tag_colors".into()));
        assert_eq!(before, after);
        std::fs::write(path, adopted).unwrap();
        local = crate::config::open(temp.path()).unwrap();
        assert_eq!(
            local.cluster.tag_colors.as_ref().unwrap(),
            &captured_style(captured).unwrap().colors
        );
        assert!(build(&local, captured).operations.is_empty());
        assert!(super::candidates(&local, captured).unwrap().is_empty());
    }
}
