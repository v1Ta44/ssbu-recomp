// Format behavior follows ARCropolis bf2005b0 modfs/handlers (GPL-3.0).
// The parsers are the same public libraries; all outputs are independent mod cache blobs.
use crate::packages::Source;
use base64::{engine::general_purpose::STANDARD, Engine};
use hash40::diff::Diff;
use serde::Deserialize;
use std::{
    collections::BTreeMap,
    fs,
    io::{BufReader, Cursor},
    path::Path,
};
#[derive(Deserialize)]
struct Xmsbt {
    #[serde(rename = "entry", default)]
    entries: Vec<XEntry>,
}
#[derive(Deserialize)]
struct XEntry {
    label: String,
    base64: Option<bool>,
    text: XText,
}
#[derive(Deserialize)]
struct XText {
    #[serde(rename = "$value", default)]
    value: String,
}
fn read_xmsbt(path: &Path) -> Result<Xmsbt, String> {
    serde_xml_rs::from_reader(fs::File::open(path).map_err(err)?).map_err(err)
}

pub(crate) fn validate_xmsbt(path: &Path) -> Result<(), String> {
    read_xmsbt(path).map(|_| ())
}

fn err(e: impl std::fmt::Debug) -> String {
    format!("{e:?}")
}
pub fn apply(kind: &str, base: Vec<u8>, patches: &[Source]) -> Result<Vec<u8>, String> {
    match kind {
        "prc" => {
            let mut value = prcx::read_stream(&mut Cursor::new(base)).map_err(err)?;
            for p in patches {
                let patch = match prcx::open(&p.disk) {
                    Ok(p) => p,
                    Err(_) => {
                        prcx::read_xml(&mut BufReader::new(fs::File::open(&p.disk).map_err(err)?))
                            .map_err(err)?
                    }
                };
                prcx::apply_patch(&patch, &mut value)
                    .map_err(|e| format!("{}: {e:?}", p.disk.display()))?;
            }
            let mut out = Cursor::new(vec![]);
            prcx::write_stream(&mut out, &value).map_err(err)?;
            Ok(out.into_inner())
        }
        "msbt" => {
            use msbt::{builder::MsbtBuilder, Msbt};
            let mut labels = BTreeMap::<String, Vec<u8>>::new();
            for p in patches {
                let patch = read_xmsbt(&p.disk)?;
                for e in patch.entries {
                    let data = if e.base64.unwrap_or(false) {
                        let mut b = STANDARD.decode(e.text.value).map_err(err)?;
                        b.extend([0, 0]);
                        b
                    } else {
                        e.text
                            .value
                            .encode_utf16()
                            .chain(Some(0))
                            .flat_map(u16::to_le_bytes)
                            .collect()
                    };
                    labels.insert(e.label, data);
                }
            }
            let mut msbt = Msbt::from_reader(Cursor::new(base)).map_err(err)?;
            for lbl in msbt.lbl1_mut().ok_or("MSBT has no LBL1")?.labels_mut() {
                if let Some(data) = labels.remove(lbl.name()) {
                    lbl.set_value_raw(data).map_err(err)?;
                }
            }
            let mut builder = MsbtBuilder::from(msbt);
            for (label, data) in labels {
                builder = builder.add_label(label, data);
            }
            let mut out = Cursor::new(vec![]);
            builder.build().write_to(&mut out).map_err(err)?;
            Ok(out.into_inner())
        }
        "audio" => {
            use nus3audio::{AudioFile, Nus3audioFile};
            let mut value = Nus3audioFile::from_bytes(&base);
            let mut files: BTreeMap<String, AudioFile> = value
                .files
                .into_iter()
                .map(|v| (v.name.clone(), v))
                .collect();
            let mut next = files
                .values()
                .map(|f| f.id)
                .max()
                .unwrap_or(0)
                .checked_add(1)
                .ok_or("audio id overflow")?;
            for p in patches {
                let bytes = fs::read(&p.disk).map_err(err)?;
                for mut af in Nus3audioFile::from_bytes(&bytes).files {
                    if let Some(old) = files.get_mut(&af.name) {
                        old.data = af.data;
                    } else {
                        af.id = next;
                        next = next.checked_add(1).ok_or("audio id overflow")?;
                        files.insert(af.name.clone(), af);
                    }
                }
            }
            value.files = files.into_values().collect();
            value.files.sort_by_key(|f| f.id);
            let mut out = vec![];
            value.write(&mut out);
            Ok(out)
        }
        "motion" => {
            let mut value = motion_lib::read_stream(&mut Cursor::new(base)).map_err(err)?;
            // ARCropolis applies complete YAML lists before motion diffs.
            for p in patches
                .iter()
                .filter(|p| p.disk.extension().is_some_and(|e| e == "yml"))
            {
                if let Some(v) =
                    serde_yaml::from_str::<Option<_>>(&fs::read_to_string(&p.disk).map_err(err)?)
                        .map_err(err)?
                {
                    value = v;
                }
            }
            for p in patches
                .iter()
                .filter(|p| p.disk.extension().is_some_and(|e| e == "motdiff"))
            {
                let diff: Option<_> =
                    serde_yaml::from_str(&fs::read_to_string(&p.disk).map_err(err)?)
                        .map_err(err)?;
                value.apply(&diff.ok_or("empty motion diff")?);
            }
            let mut out = Cursor::new(vec![]);
            motion_lib::write_stream(&mut out, &value).map_err(err)?;
            Ok(out.into_inner())
        }
        "bgm" => {
            use smash_bgm_property::BgmPropertyFile;
            let mut value = BgmPropertyFile::read(&mut Cursor::new(base)).map_err(err)?;
            for p in patches {
                let mut other = BgmPropertyFile::from_file(&p.disk).map_err(err)?;
                value.entries.append(&mut other.entries);
            }
            let mut out = Cursor::new(vec![]);
            value.write(&mut out).map_err(err)?;
            Ok(out.into_inner())
        }
        _ => Err(format!("unknown patch handler {kind}")),
    }
}
