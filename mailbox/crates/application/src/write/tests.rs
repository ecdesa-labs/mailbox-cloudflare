use mailbox_domain::UnixMs;
use serde_json::json;

use super::*;

fn id(n: u128) -> MessageId {
    MessageId::from_parts(UnixMs::new(0).unwrap(), n)
}

fn rid() -> RuleId {
    RuleId::from_parts(UnixMs::new(0).unwrap(), 1)
}

#[test]
fn patch_valido() {
    let p = MessagePatch::from_json(&json!({ "read": true, "folder": "archive" })).unwrap();
    assert_eq!(
        p,
        MessagePatch {
            read: Some(true),
            starred: None,
            folder: Some(Folder::Archive)
        }
    );
    let p = MessagePatch::from_json(&json!({ "starred": false, "read": null })).unwrap();
    assert_eq!((p.read, p.starred), (None, Some(false)));
}

#[test]
fn patch_invalido() {
    for v in [
        json!({}),
        json!(null),
        json!([]),
        json!({ "read": "sim" }),
        json!({ "folder": "rascunhos" }),
        json!({ "folder": 1 }),
        json!({ "lido": true }),
        json!({ "read": true, "deleted_at": 1 }),
    ] {
        assert!(MessagePatch::from_json(&v).is_err(), "{v}");
    }
}

#[test]
fn lote() {
    let a = id(1).to_string();
    let b = id(2).to_string();
    let bulk =
        BulkPatch::from_json(&json!({ "ids": [a, b, a], "patch": { "read": true } })).unwrap();
    assert_eq!(bulk.ids, [id(1), id(2)], "sem repetidos");
    assert_eq!(bulk.patch.read, Some(true));

    let many: Vec<String> = (0..101).map(|n| id(n).to_string()).collect();
    for v in [
        json!({ "ids": [], "patch": { "read": true } }),
        json!({ "ids": many, "patch": { "read": true } }),
        json!({ "ids": ["lixo"], "patch": { "read": true } }),
        json!({ "ids": [a], "patch": {} }),
        json!({ "ids": [a] }),
        json!({ "ids": a, "patch": { "read": true } }),
        json!({ "ids": [a], "patch": { "read": true }, "extra": 1 }),
    ] {
        assert!(BulkPatch::from_json(&v).is_err(), "{v}");
    }
}

#[test]
fn regra_nova() {
    let r = rule_from_json(
        rid(),
        &json!({ "match_address": "*@Pessoal.dev", "action": "move", "folder": "spam" }),
    )
    .unwrap();
    assert_eq!(r.pattern.to_string(), "*@pessoal.dev");
    assert_eq!(r.action, RuleAction::Move(Folder::Spam));
    assert_eq!((r.priority, r.enabled), (DEFAULT_PRIORITY, true));
    assert_eq!(rule_json(&r)["folder"], "spam");

    let r = rule_from_json(rid(), &json!({ "match_address": "vazou@pessoal.dev", "action": "reject", "priority": 1, "enabled": false })).unwrap();
    assert_eq!(
        (r.action, r.priority, r.enabled),
        (RuleAction::Reject, 1, false)
    );
    assert_eq!(rule_json(&r)["folder"], serde_json::Value::Null);
}

#[test]
fn regra_invalida() {
    for v in [
        json!({ "action": "archive" }),
        json!({ "match_address": "*@x.dev" }),
        json!({ "match_address": "*", "action": "archive" }),
        json!({ "match_address": "*@x.dev", "action": "move" }),
        json!({ "match_address": "*@x.dev", "action": "archive", "folder": "spam" }),
        json!({ "match_address": "*@x.dev", "action": "delete" }),
        json!({ "match_address": "*@x.dev", "action": "archive", "priority": "alta" }),
        json!({ "match_address": "*@x.dev", "action": "archive", "cor": "azul" }),
    ] {
        assert!(rule_from_json(rid(), &v).is_err(), "{v}");
    }
}

#[test]
fn alterar_regra() {
    let r = rule_from_json(
        rid(),
        &json!({ "match_address": "*@x.dev", "action": "move", "folder": "spam" }),
    )
    .unwrap();
    // Só a pasta: continua `move`.
    let r2 = apply_rule_patch(r.clone(), &json!({ "folder": "archive" })).unwrap();
    assert_eq!(r2.action, RuleAction::Move(Folder::Archive));
    // Trocar para uma ação sem pasta descarta a pasta.
    let r3 = apply_rule_patch(r.clone(), &json!({ "action": "mark_read" })).unwrap();
    assert_eq!(r3.action, RuleAction::MarkRead);
    // Só desabilitar mantém o resto.
    let r4 = apply_rule_patch(r.clone(), &json!({ "enabled": false })).unwrap();
    assert_eq!((r4.enabled, r4.action, r4.id), (false, r.action, r.id));
    // Inválidos.
    assert!(apply_rule_patch(r.clone(), &json!({})).is_err());
    assert!(apply_rule_patch(r.clone(), &json!({ "action": "move" })).is_err());
    assert!(apply_rule_patch(r, &json!({ "id": "outro" })).is_err());
}
