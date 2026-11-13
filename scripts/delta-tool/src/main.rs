// The release side of in-app updates. scripts/delta.mjs runs it in the Windows
// release jobs:
//
//   delta-tool pack <installer> <out.zst>          the download, packed
//   delta-tool pack-at <level> <installer> <out>   the same, at another level
//   delta-tool unpack <in.zst> <out>               the installer back, as built
//   delta-tool make-patch <base> <new> <out.zst>   the patch from base to new
//   delta-tool apply-patch <base> <patch> <out>    the check: the new installer
//
// The codec is the shell's own file (src-tauri/src/delta_codec.rs), so a patch
// made here is applied by exactly the code the app runs.
#[path = "../../../src-tauri/src/delta_codec.rs"]
mod delta_codec;

use std::fs;
use std::process::ExitCode;

fn run(args: &[String]) -> Result<(), String> {
    let [command, rest @ ..] = args else {
        return Err(USAGE.into());
    };
    let read = |path: &str| fs::read(path).map_err(|e| format!("{path}: {e}"));
    let write =
        |path: &str, bytes: &[u8]| fs::write(path, bytes).map_err(|e| format!("{path}: {e}"));
    let fail = |e: std::io::Error| e.to_string();
    match (command.as_str(), rest) {
        ("pack", [input, out]) => {
            let packed = delta_codec::pack(&read(input)?, delta_codec::PACK_LEVEL).map_err(fail)?;
            write(out, &packed)
        }
        ("pack-at", [level, input, out]) => {
            let level: i32 = level
                .parse()
                .map_err(|_| format!("not a zstd level: {level}"))?;
            let packed = delta_codec::pack(&read(input)?, level).map_err(fail)?;
            write(out, &packed)
        }
        ("unpack", [input, out]) => {
            let raw = delta_codec::unpack(&read(input)?).map_err(fail)?;
            write(out, &raw)
        }
        ("make-patch", [base, new, out]) => {
            let patch = delta_codec::make_patch(&read(base)?, &read(new)?).map_err(fail)?;
            write(out, &patch)
        }
        ("apply-patch", [base, patch, out]) => {
            let raw = delta_codec::apply_patch(&read(base)?, &read(patch)?).map_err(fail)?;
            write(out, &raw)
        }
        _ => Err(USAGE.into()),
    }
}

const USAGE: &str = "usage: delta-tool pack <in> <out> | pack-at <level> <in> <out> | unpack <in> <out> | make-patch|apply-patch <base> <in> <out>";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("delta-tool: {message}");
            ExitCode::FAILURE
        }
    }
}
