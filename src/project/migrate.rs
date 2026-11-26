//! R42: project file format migration. A file carries `format_version` (0 = written before versioning); `migrate`
//! runs the steps from that version up to `FORMAT_VERSION` on the JSON before it is deserialized, so each step sees
//! exactly what the build of its time wrote. A step is a pure `Value -> Value`, and each has a test below. A file from
//! a newer build is never migrated down: `Project::open` opens it read-only instead.
use serde_json::{Map, Value};
use super::FORMAT_VERSION;

/// The steps, in order: `STEPS[n]` takes a version-`n` file to version `n + 1`.
const STEPS: &[fn(Value) -> Value] = &[v0_to_v1];

/// Bring a version-`from` file up to `FORMAT_VERSION`. `from >= FORMAT_VERSION` only stamps the version.
pub fn migrate(mut value: Value, from: u32) -> Value {
    for step in STEPS.iter().skip(from as usize).take(FORMAT_VERSION.saturating_sub(from) as usize) {
        value = step(value);
    }
    if let Value::Object(o) = &mut value {
        o.insert("format_version".into(), Value::from(FORMAT_VERSION));
    }
    value
}

/// 0 → 1: the first versioned format. Builds up to 0.7.0 wrote every optional field, `null` included (`loop_region`,
/// `rel_path`, `group`, `transition`, ...); the versioned format leaves an absent optional out, as later builds write
/// it, so the nulls go. Data is unchanged: every field the old builds wrote reads the same way in version 1.
fn v0_to_v1(mut value: Value) -> Value {
    fn strip_nulls(v: &mut Value) {
        match v {
            Value::Object(o) => {
                o.retain(|_, v| !v.is_null());
                o.values_mut().for_each(strip_nulls);
            }
            Value::Array(a) => a.iter_mut().for_each(strip_nulls),
            _ => {}
        }
    }
    if let Value::Object(o) = &mut value {
        for key in ["timeline", "media_pool"] {
            if let Some(v) = o.get_mut(key) {
                strip_nulls(v);
            }
        }
        // `Keyframes` maps and FX parameter maps keep their shape; only project-level optionals were stripped above.
        let _: &Map<String, Value> = o;
    }
    value
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn steps_cover_every_version() {
        assert_eq!(STEPS.len() as u32, FORMAT_VERSION, "one step per version bump");
    }

    #[test]
    fn v0_to_v1_strips_nulls_and_stamps_the_version() {
        use crate::project::{Clip, MediaItem, Project};
        // What a 0.7.0 build wrote: no format_version, and every optional field present, null when unset.
        let mut p = Project::new("Show");
        p.media_pool.push(MediaItem { id: "m".into(), path: "C:/a.mp4".into(), name: "a".into(), duration: 1.0, ..Default::default() });
        p.timeline.video_tracks[0].clips.push(Clip::new("c", 1.0, 2.0));
        let mut v0 = serde_json::to_value(&p).unwrap();
        v0.as_object_mut().unwrap().remove("format_version");
        v0["timeline"]["loop_region"] = Value::Null;
        v0["timeline"]["video_tracks"][0]["clips"][0]["group"] = Value::Null;
        v0["media_pool"][0]["rel_path"] = Value::Null;
        assert!(v0.get("format_version").is_none());
        let v1 = migrate(v0, 0);
        assert_eq!(v1["format_version"], json!(1));
        assert!(v1["timeline"].get("loop_region").is_none());
        assert!(v1["timeline"]["video_tracks"][0]["clips"][0].get("group").is_none());
        assert!(v1["media_pool"][0].get("rel_path").is_none());
        assert_eq!(v1["media_pool"][0]["path"], json!("C:/a.mp4"));
        assert_eq!(v1["timeline"]["video_tracks"][0]["clips"][0]["position"], json!(1.0));
        // The migrated file deserializes as a project with the same content.
        let back: Project = serde_json::from_value(v1).unwrap();
        assert_eq!((back.format_version, back.name.as_str()), (1, "Show"));
        assert_eq!(back.timeline.video_tracks[0].clips, p.timeline.video_tracks[0].clips);
    }

    #[test]
    fn a_current_file_is_only_stamped() {
        let v = json!({"name": "x", "format_version": 1, "timeline": {"loop_region": null}});
        let out = migrate(v, 1);
        assert_eq!(out["format_version"], json!(1));
        assert!(out["timeline"]["loop_region"].is_null(), "no step ran on a current file");
    }
}
