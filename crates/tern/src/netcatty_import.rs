//! Parse a Netcatty JSON export (Sanitized Export or host list) into tern `Connection`s.
//! Encrypted Netcatty backups (`safeStorage-v1`) are detected and refused with a clear hint.

use std::collections::HashMap;

use serde_json::Value;

use crate::connections::Connection;
use crate::i18n::tr;

/// Parses Netcatty JSON text into SSH connections.
pub fn parse_netcatty_json(text: &str) -> Result<Vec<Connection>, String> {
    let root: Value = serde_json::from_str(text)
        .map_err(|e| format!("{}: {e}", tr("Could not read Netcatty JSON")))?;

    if is_encrypted_backup(&root) {
        return Err(tr(
            "This is an encrypted Netcatty backup. In Netcatty, use Settings → Data → Export Vault → Sanitized Export (JSON).",
        )
        .to_owned());
    }

    let payload = unwrap_container(&root);
    let groups = collect_groups(payload);
    let hosts_val = extract_hosts_array(payload)
        .ok_or_else(|| tr("No hosts array found in this Netcatty JSON file.").to_owned())?;

    let mut out = Vec::new();
    for item in hosts_val {
        if let Some(conn) = parse_host(item, &groups) {
            out.push(conn);
        }
    }
    Ok(out)
}

fn is_encrypted_backup(root: &Value) -> bool {
    let Some(obj) = root.as_object() else {
        return false;
    };
    obj.get("payloadEncoding").is_some()
        || (obj.get("payloadData").is_some() && obj.get("hosts").is_none())
}

fn unwrap_container(root: &Value) -> &Value {
    if let Some(obj) = root.as_object() {
        for key in ["data", "vault", "payload"] {
            if let Some(inner) = obj.get(key)
                && inner.is_object()
                && (inner.get("hosts").is_some() || inner.get("groups").is_some())
            {
                return inner;
            }
        }
    }
    root
}

fn extract_hosts_array(payload: &Value) -> Option<&Vec<Value>> {
    if let Some(arr) = payload.as_array() {
        return Some(arr);
    }
    payload.as_object()?.get("hosts")?.as_array()
}

fn collect_groups(payload: &Value) -> HashMap<String, String> {
    let mut raw: HashMap<String, (String, Option<String>)> = HashMap::new();
    let Some(obj) = payload.as_object() else {
        return HashMap::new();
    };
    let arr = obj
        .get("groups")
        .or_else(|| obj.get("folders"))
        .and_then(Value::as_array);
    let Some(arr) = arr else {
        return HashMap::new();
    };
    for g in arr {
        let Some(gobj) = g.as_object() else {
            continue;
        };
        let Some(id) = first_str(gobj, &["id", "groupId"]) else {
            continue;
        };
        let Some(name) = first_str(gobj, &["name", "label", "title"]) else {
            continue;
        };
        let parent = first_str(gobj, &["parentId", "parent"]).map(str::to_owned);
        raw.insert(id.to_owned(), (name.to_owned(), parent));
    }

    let mut resolved = HashMap::with_capacity(raw.len());
    for id in raw.keys() {
        let mut parts = Vec::new();
        let mut cur = Some(id.as_str());
        let mut depth = 0;
        while let Some(cid) = cur {
            if depth >= 8 {
                break;
            }
            depth += 1;
            let Some((name, parent)) = raw.get(cid) else {
                break;
            };
            parts.push(name.as_str());
            cur = parent.as_deref();
        }
        if !parts.is_empty() {
            parts.reverse();
            resolved.insert(id.clone(), parts.join("/"));
        }
    }
    resolved
}

fn parse_host(val: &Value, groups: &HashMap<String, String>) -> Option<Connection> {
    let obj = val.as_object()?;

    if let Some(proto) = first_str(obj, &["protocol", "type"])
        && !proto.eq_ignore_ascii_case("ssh")
    {
        return None;
    }

    let host = first_str(obj, &["hostname", "host", "address", "ip"])?.trim();
    if host.is_empty() {
        return None;
    }
    let name = first_str(obj, &["label", "name", "alias"])
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(host);

    let port = obj
        .get("port")
        .and_then(|p| {
            p.as_u64()
                .and_then(|n| u16::try_from(n).ok())
                .or_else(|| p.as_str().and_then(|s| s.trim().parse::<u16>().ok()))
        })
        .filter(|&p| p > 0)
        .unwrap_or(22);

    let user = first_str(obj, &["username", "user"])
        .map(str::trim)
        .unwrap_or_default()
        .to_owned();

    let identity_file = first_str(obj, &["identityFile", "keyPath", "privateKeyPath"])
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned);

    let group = first_str(obj, &["group", "groupName", "folder"])
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .or_else(|| {
            first_str(obj, &["groupId"])
                .and_then(|id| groups.get(id))
                .cloned()
        });

    let tags = parse_tags(obj.get("tags"));

    let proxy_jump = first_str(obj, &["proxyJump", "jumpHost"])
        .map(str::trim)
        .unwrap_or_default()
        .to_owned();

    let forward_agent = obj
        .get("agentForwarding")
        .or_else(|| obj.get("forwardAgent"))
        .and_then(Value::as_bool)
        .unwrap_or(false);

    let keep_alive = obj
        .get("keepAliveInterval")
        .or_else(|| obj.get("keepAlive"))
        .and_then(Value::as_u64)
        .and_then(|n| u32::try_from(n).ok());

    Some(Connection {
        name: name.to_owned(),
        host: host.to_owned(),
        port,
        user,
        identity_file,
        group,
        tags,
        proxy_jump,
        forward_agent,
        keep_alive,
        ..Connection::default()
    })
}

fn first_str<'a>(obj: &'a serde_json::Map<String, Value>, keys: &[&str]) -> Option<&'a str> {
    keys.iter().find_map(|k| obj.get(*k).and_then(Value::as_str))
}

fn parse_tags(val: Option<&Value>) -> Vec<String> {
    let Some(val) = val else {
        return Vec::new();
    };
    if let Some(arr) = val.as_array() {
        return arr
            .iter()
            .filter_map(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .collect();
    }
    if let Some(s) = val.as_str() {
        return s
            .split(',')
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .map(str::to_owned)
            .collect();
    }
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_sanitized_netcatty_export_with_groups_and_tags() {
        let json = r#"{
            "groups": [
                { "id": "g1", "name": "Prod" },
                { "id": "g2", "name": "DB", "parentId": "g1" }
            ],
            "hosts": [
                {
                    "id": "h1",
                    "label": "api-1",
                    "hostname": "10.0.0.10",
                    "port": 2222,
                    "username": "deploy",
                    "groupId": "g2",
                    "tags": ["web", "eu"],
                    "protocol": "ssh",
                    "agentForwarding": true,
                    "keepAliveInterval": 60
                },
                {
                    "id": "h2",
                    "label": "local-shell",
                    "hostname": "localhost",
                    "protocol": "local"
                }
            ]
        }"#;
        let out = parse_netcatty_json(json).expect("valid export");
        assert_eq!(out.len(), 1);
        let c = &out[0];
        assert_eq!(c.name, "api-1");
        assert_eq!(c.host, "10.0.0.10");
        assert_eq!(c.port, 2222);
        assert_eq!(c.user, "deploy");
        assert_eq!(c.group.as_deref(), Some("Prod/DB"));
        assert_eq!(c.tags, vec!["web", "eu"]);
        assert!(c.forward_agent);
        assert_eq!(c.keep_alive, Some(60));
    }

    #[test]
    fn refuses_encrypted_safe_storage_backup_with_clear_hint() {
        let json = r#"{
            "formatVersion": 1,
            "payloadEncoding": "safeStorage-v1",
            "payloadData": "djEwuOJSpLz+"
        }"#;
        let err = parse_netcatty_json(json).expect_err("should refuse encrypted backup");
        assert!(err.contains("Sanitized Export"));
    }

    #[test]
    fn parses_bare_host_array_and_defaults_port_and_name() {
        let json = r#"[
            { "hostname": "192.168.1.50", "username": "root", "group": "Lab" }
        ]"#;
        let out = parse_netcatty_json(json).expect("array export");
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].name, "192.168.1.50");
        assert_eq!(out[0].port, 22);
        assert_eq!(out[0].group.as_deref(), Some("Lab"));
    }
}
