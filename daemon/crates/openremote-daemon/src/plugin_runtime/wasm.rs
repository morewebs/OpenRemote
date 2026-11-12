//! WASM plugins: agent tools compiled to WebAssembly, run inside the daemon
//! on wasmi, a small pure-Rust interpreter. A module is sandboxed: it sees
//! its own memory and the host functions its approved powers grant -
//! nothing else. There is no WASI: no files, no environment, no clock, no
//! sockets. Every call runs on a fresh instance (no state carries between
//! calls or chats), under a fuel limit (CPU), a memory limit and a deadline
//! for its requests, so a plugin can't hang or swell the daemon.
//!
//! The contract:
//! - the module exports `memory`, `or_alloc(len: i32) -> i32` and
//!   `or_call(ptr: i32, len: i32) -> i64`;
//! - OpenRemote writes the call - `{"tool", "arguments", "settings",
//!   "chat"}` as JSON - into memory `or_alloc` gave it and calls
//!   `or_call`, which answers `(ptr << 32) | len` of its result: an MCP
//!   tool result (`{"content": [...], "isError": bool}`) as JSON;
//! - imports come from the module `openremote`: `log(ptr, len)` always;
//!   `http(ptr, len) -> i32` and `take(ptr, len) -> i32` only with the
//!   network power. A module that imports anything else is refused at
//!   review and cannot start.

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::{Duration, Instant, SystemTime};

use openremote_core::PluginPower;
use serde_json::{Value, json};
use wasmi::{
    Caller, Config, Engine, Extern, Linker, Module, Store, StoreLimits, StoreLimitsBuilder,
};

/// Instructions a call may run - a few seconds of work at most.
const FUEL: u64 = 2_000_000_000;
/// The most memory one call's instance may hold.
const MEMORY: usize = 64 * 1024 * 1024;
/// The most table elements one call's instance may hold.
const TABLE_ELEMENTS: usize = 100_000;
/// A call's input and output are capped here.
const MAX_IO: usize = 16 * 1024 * 1024;
/// Requests one call may make, each with its own time limit, all inside
/// the call's deadline.
const MAX_REQUESTS: u32 = 16;
const REQUEST_TIME: Duration = Duration::from_secs(30);
/// A module is refused past this size.
const MAX_MODULE: u64 = 32 * 1024 * 1024;
/// Calls running at once: each takes a blocking thread.
const PARALLEL: usize = 4;
/// Request headers a module may not set: the connection is the daemon's.
const RESERVED_HEADERS: &[&str] = &[
    "host",
    "content-length",
    "transfer-encoding",
    "connection",
    "upgrade",
    "te",
    "trailer",
    "keep-alive",
    "proxy-authorization",
    "proxy-connection",
];

/// The imports a module may name, and the power each needs.
const IMPORTS: &[(&str, Option<PluginPower>)] = &[
    ("log", None),
    ("http", Some(PluginPower::Network)),
    ("take", Some(PluginPower::Network)),
];

struct Host {
    limits: StoreLimits,
    plugin: String,
    requests_left: u32,
    /// After this the module's requests are refused: the call is over.
    deadline: Instant,
    /// The last host answer, waiting for the module to `take` it.
    held: Vec<u8>,
    http: openremote_cloud::https::Http,
    runtime: tokio::runtime::Handle,
}

/// Compiled modules, by file and when it last changed.
pub struct WasmHost {
    engine: Engine,
    modules: StdMutex<HashMap<PathBuf, (Option<SystemTime>, Module)>>,
    parallel: Arc<tokio::sync::Semaphore>,
    fuel: u64,
}

impl Default for WasmHost {
    fn default() -> Self {
        Self::with_fuel(FUEL)
    }
}

fn engine() -> Engine {
    let mut config = Config::default();
    config.consume_fuel(true);
    Engine::new(&config)
}

impl WasmHost {
    /// A host whose calls get `fuel` instructions (the tests use less, so
    /// a module that never stops is stopped quickly).
    pub fn with_fuel(fuel: u64) -> Self {
        Self::with_limits(fuel, PARALLEL)
    }

    fn with_limits(fuel: u64, parallel: usize) -> Self {
        Self {
            engine: engine(),
            modules: StdMutex::new(HashMap::new()),
            parallel: Arc::new(tokio::sync::Semaphore::new(parallel)),
            fuel,
        }
    }

    fn module(&self, path: &Path) -> Result<Module, String> {
        let modified = std::fs::metadata(path).and_then(|m| m.modified()).ok();
        if let Some((seen, module)) = self.modules.lock().ok().and_then(|m| m.get(path).cloned()) {
            if seen == modified {
                return Ok(module);
            }
        }
        let module = compile(&self.engine, path)?;
        if let Ok(mut modules) = self.modules.lock() {
            modules.insert(path.to_path_buf(), (modified, module.clone()));
        }
        Ok(module)
    }

    /// Run one tool call: `input` is the call as the module reads it,
    /// `powers` are the ones the user approved at install, `time` is how
    /// long the call's requests may go on. Always answers an MCP tool
    /// result - a failure is an error result the agent reads.
    pub async fn call(
        &self,
        plugin: &str,
        path: &Path,
        powers: &[PluginPower],
        input: Value,
        time: Duration,
    ) -> Value {
        let module = match self.module(path) {
            Ok(module) => module,
            Err(e) => return error_result(&e),
        };
        let deadline = Instant::now() + time;
        // A turn waits no longer than the call may take: slow tools of one
        // plugin can't hold the others back forever.
        let turn = tokio::time::timeout_at(
            tokio::time::Instant::from_std(deadline),
            Arc::clone(&self.parallel).acquire_owned(),
        )
        .await;
        let _turn = match turn {
            Ok(Ok(turn)) => turn,
            Ok(Err(_)) => return error_result("OpenRemote is stopping"),
            Err(_) => {
                return error_result(
                    "OpenRemote is running other plugin tools; this one waited past its time limit",
                );
            }
        };
        let engine = self.engine.clone();
        let plugin = plugin.to_string();
        let network = powers.contains(&PluginPower::Network);
        let fuel = self.fuel;
        let runtime = tokio::runtime::Handle::current();
        let run = tokio::task::spawn_blocking(move || {
            run(
                &engine, &module, &plugin, network, fuel, deadline, &input, runtime,
            )
        });
        match run.await {
            Ok(Ok(result)) => result,
            Ok(Err(e)) => error_result(&e),
            Err(_) => error_result("the plugin's module stopped unexpectedly"),
        }
    }
}

fn compile(engine: &Engine, path: &Path) -> Result<Module, String> {
    let size = std::fs::metadata(path)
        .map_err(|_| format!("{} is missing", path.display()))?
        .len();
    if size > MAX_MODULE {
        return Err("the module is too large".into());
    }
    let bytes = std::fs::read(path).map_err(|e| format!("can't read the module: {e}"))?;
    Module::new(engine, &bytes).map_err(|e| format!("the module is not valid WebAssembly: {e}"))
}

/// At review: the module compiles, exports what OpenRemote calls, and
/// imports only what its powers allow.
pub fn check(path: &Path, powers: &[PluginPower]) -> Result<(), String> {
    let module = compile(&engine(), path)?;
    for import in module.imports() {
        let name = import.name();
        let allowed = import.module() == "openremote"
            && IMPORTS
                .iter()
                .any(|(n, power)| *n == name && power.is_none_or(|p| powers.contains(&p)));
        if !allowed {
            let needs = IMPORTS
                .iter()
                .find(|(n, _)| *n == name)
                .and_then(|(_, p)| *p);
            return Err(match (import.module(), needs) {
                ("openremote", Some(power)) => format!(
                    "the module imports openremote.{name}, which needs the {} power its manifest doesn't ask for",
                    serde_json::to_value(power)
                        .ok()
                        .and_then(|v| v.as_str().map(String::from))
                        .unwrap_or_default()
                ),
                (module, _) => format!(
                    "the module imports {module}.{name}, which OpenRemote doesn't offer (a WASM plugin gets only openremote.log, http and take)"
                ),
            });
        }
    }
    let exports: Vec<&str> = module.exports().map(|e| e.name()).collect();
    for wanted in ["memory", "or_alloc", "or_call"] {
        if !exports.contains(&wanted) {
            return Err(format!(
                "the module doesn't export {wanted}; a tool module exports memory, or_alloc and or_call"
            ));
        }
    }
    Ok(())
}

fn trap(message: impl Into<String>) -> wasmi::Error {
    wasmi::Error::new(message.into())
}

fn memory_of(caller: &Caller<'_, Host>) -> Result<wasmi::Memory, wasmi::Error> {
    caller
        .get_export("memory")
        .and_then(Extern::into_memory)
        .ok_or_else(|| trap("the module exports no memory"))
}

fn read_guest(caller: &Caller<'_, Host>, ptr: i32, len: i32) -> Result<Vec<u8>, wasmi::Error> {
    let len = usize::try_from(len).map_err(|_| trap("a negative length"))?;
    if len > MAX_IO {
        return Err(trap("too much at once"));
    }
    let mut buf = vec![0; len];
    memory_of(caller)?
        .read(caller, ptr as u32 as usize, &mut buf)
        .map_err(|_| trap("out of the module's memory"))?;
    Ok(buf)
}

/// The linker for one call: `log` always, the network's two functions
/// only when the user approved the network power. Nothing else exists to
/// import - no WASI, so no files, environment, clock or sockets.
fn linker(engine: &Engine, network: bool) -> Result<Linker<Host>, String> {
    let mut linker = <Linker<Host>>::new(engine);
    linker
        .func_wrap(
            "openremote",
            "log",
            |caller: Caller<'_, Host>, ptr: i32, len: i32| -> Result<(), wasmi::Error> {
                let bytes = read_guest(&caller, ptr, len.min(4096))?;
                eprintln!(
                    "plugin {}: {}",
                    caller.data().plugin,
                    String::from_utf8_lossy(&bytes)
                );
                Ok(())
            },
        )
        .map_err(|e| e.to_string())?;
    if network {
        linker
            .func_wrap(
                "openremote",
                "http",
                |mut caller: Caller<'_, Host>, ptr: i32, len: i32| -> Result<i32, wasmi::Error> {
                    let request = read_guest(&caller, ptr, len)?;
                    let answer = http(caller.data_mut(), &request);
                    let bytes = serde_json::to_vec(&answer).expect("an answer serializes");
                    let size = bytes.len() as i32;
                    caller.data_mut().held = bytes;
                    Ok(size)
                },
            )
            .map_err(|e| e.to_string())?;
        linker
            .func_wrap(
                "openremote",
                "take",
                |mut caller: Caller<'_, Host>, ptr: i32, len: i32| -> Result<i32, wasmi::Error> {
                    let held = std::mem::take(&mut caller.data_mut().held);
                    let n = held.len().min(usize::try_from(len).unwrap_or(0));
                    memory_of(&caller)?
                        .write(&mut caller, ptr as u32 as usize, &held[..n])
                        .map_err(|_| trap("out of the module's memory"))?;
                    Ok(n as i32)
                },
            )
            .map_err(|e| e.to_string())?;
    }
    Ok(linker)
}

#[allow(clippy::too_many_arguments)]
fn run(
    engine: &Engine,
    module: &Module,
    plugin: &str,
    network: bool,
    fuel: u64,
    deadline: Instant,
    input: &Value,
    runtime: tokio::runtime::Handle,
) -> Result<Value, String> {
    let host = Host {
        limits: StoreLimitsBuilder::new()
            .memory_size(MEMORY)
            .table_elements(TABLE_ELEMENTS)
            .instances(1)
            .tables(4)
            .memories(1)
            .build(),
        plugin: plugin.to_string(),
        requests_left: MAX_REQUESTS,
        deadline,
        held: Vec::new(),
        http: openremote_cloud::https::Http::new(),
        runtime,
    };
    let mut store = Store::new(engine, host);
    store.limiter(|host| &mut host.limits);
    store
        .set_fuel(fuel)
        .map_err(|e| format!("fuel could not be set: {e}"))?;
    let linker = linker(engine, network)?;
    let instance = linker
        .instantiate(&mut store, module)
        .and_then(|pre| pre.start(&mut store))
        .map_err(|e| format!("the module could not start: {}", ran_out(&e)))?;
    let memory = instance
        .get_memory(&store, "memory")
        .ok_or("the module exports no memory")?;
    let alloc = instance
        .get_typed_func::<i32, i32>(&store, "or_alloc")
        .map_err(|_| "the module's or_alloc is missing or not (i32) -> i32")?;
    let call = instance
        .get_typed_func::<(i32, i32), i64>(&store, "or_call")
        .map_err(|_| "the module's or_call is missing or not (i32, i32) -> i64")?;
    let bytes = serde_json::to_vec(input).expect("a call serializes");
    if bytes.len() > MAX_IO {
        return Err("the call is too large for the tool".into());
    }
    let ptr = alloc
        .call(&mut store, bytes.len() as i32)
        .map_err(|e| ran_out(&e))?;
    memory
        .write(&mut store, ptr as u32 as usize, &bytes)
        .map_err(|_| "or_alloc gave memory the module doesn't have")?;
    let packed = call
        .call(&mut store, (ptr, bytes.len() as i32))
        .map_err(|e| ran_out(&e))?;
    let (out_ptr, out_len) = (
        (packed as u64 >> 32) as usize,
        (packed as u64 & 0xffff_ffff) as usize,
    );
    if out_len > MAX_IO {
        return Err("the tool answered too much".into());
    }
    let mut out = vec![0; out_len];
    memory
        .read(&store, out_ptr, &mut out)
        .map_err(|_| "the tool's answer is outside its memory")?;
    let answer: Value =
        serde_json::from_slice(&out).map_err(|_| "the tool's answer is not JSON".to_string())?;
    Ok(super::tool_result(answer))
}

/// Why a call stopped, in plain words: the limits get their own.
fn ran_out(e: &wasmi::Error) -> String {
    let text = e.to_string();
    if text.contains("fuel") {
        "the tool ran past its time limit and was stopped".into()
    } else if text.contains("memory") || text.contains("limit") || text.contains("resource") {
        format!("the tool ran past its memory limit and was stopped ({text})")
    } else {
        format!("the tool stopped with an error: {text}")
    }
}

/// One request through the daemon's own HTTP client, for a module with
/// the network power: the internet only - never this machine or its
/// network's private addresses - to the very address that was checked,
/// inside the call's deadline, never redirected.
fn http(host: &mut Host, raw: &[u8]) -> Value {
    if host.requests_left == 0 {
        return json!({"error": format!("a call may make {MAX_REQUESTS} requests at most")});
    }
    let left = host.deadline.saturating_duration_since(Instant::now());
    if left.is_zero() {
        return json!({"error": "the call's time is up: no more requests"});
    }
    host.requests_left -= 1;
    let Ok(request) = serde_json::from_slice::<Value>(raw) else {
        return json!({"error": "the request is not JSON"});
    };
    let method = request["method"].as_str().unwrap_or("GET").to_uppercase();
    if !method.chars().all(|c| c.is_ascii_uppercase()) || method.is_empty() || method.len() > 16 {
        return json!({"error": "the request's method is not one"});
    }
    let Some(url) = request["url"].as_str().map(String::from) else {
        return json!({"error": "the request names no url"});
    };
    let headers: Vec<(String, String)> = request["headers"]
        .as_object()
        .map(|h| {
            h.iter()
                .filter(|(k, _)| !RESERVED_HEADERS.contains(&k.to_ascii_lowercase().as_str()))
                .filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_string())))
                .collect()
        })
        .unwrap_or_default();
    let body = request["body"].as_str().map(|text| {
        let kind = request["content_type"]
            .as_str()
            .unwrap_or("text/plain; charset=utf-8")
            .to_string();
        (kind, text.as_bytes().to_vec())
    });
    let http = host.http.clone();
    // The lookup and the request together get one time limit: the call's
    // time left, at most REQUEST_TIME. A name that never resolves can't
    // hold the call past its deadline.
    let limit = left.min(REQUEST_TIME);
    let answer = host.runtime.block_on(within(limit, async move {
        let addr = public_address(&url).await?;
        let header_refs: Vec<(&str, &str)> = headers
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect();
        http.request_to(
            limit,
            addr,
            &method,
            &url,
            &header_refs,
            body.as_ref().map(|(k, b)| (k.as_str(), b.clone())),
        )
        .await
        .map_err(|e| e.to_string())
    }));
    match answer {
        Ok(response) => {
            let headers: serde_json::Map<String, Value> = response
                .headers
                .iter()
                .filter_map(|(k, v)| Some((k.to_string(), Value::String(v.to_str().ok()?.into()))))
                .collect();
            match String::from_utf8(response.body) {
                Ok(text) => json!({"status": response.status, "headers": headers, "body": text}),
                Err(raw) => {
                    use base64::Engine as _;
                    json!({
                        "status": response.status,
                        "headers": headers,
                        "body_base64": base64::engine::general_purpose::STANDARD.encode(raw.into_bytes()),
                    })
                }
            }
        }
        Err(why) => json!({"error": why}),
    }
}

/// `work`, ended at `limit` with the words a module reads.
async fn within<T>(
    limit: Duration,
    work: impl std::future::Future<Output = Result<T, String>>,
) -> Result<T, String> {
    tokio::time::timeout(limit, work).await.unwrap_or_else(|_| {
        Err(format!(
            "the request took longer than {}s",
            limit.as_secs_f32()
        ))
    })
}

/// An http(s) address that leads to the public internet, and the one
/// address the request will connect to. A name is looked up once: if any
/// answer is a loopback, private, link-local or otherwise special address
/// (this machine, the home network, a cloud's metadata service) it is
/// refused, and the connection goes to the vetted address - never to a
/// second lookup's.
async fn public_address(url: &str) -> Result<SocketAddr, String> {
    let uri: axum::http::Uri = url
        .parse()
        .map_err(|_| format!("'{url}' is not an address"))?;
    let secure = match uri.scheme_str() {
        Some("https") => true,
        Some("http") => false,
        _ => return Err("a request goes to an http or https address".into()),
    };
    let host = uri
        .host()
        .ok_or("the address names no host")?
        .trim_start_matches('[')
        .trim_end_matches(']')
        .to_string();
    if host.eq_ignore_ascii_case("localhost") || host.to_ascii_lowercase().ends_with(".localhost") {
        return Err("a plugin reaches the internet, not this machine".into());
    }
    let port = uri.port_u16().unwrap_or(if secure { 443 } else { 80 });
    let addresses: Vec<SocketAddr> = match host.parse::<IpAddr>() {
        Ok(ip) => vec![SocketAddr::new(ip, port)],
        Err(_) => tokio::net::lookup_host((host.as_str(), port))
            .await
            .map_err(|_| format!("{host} could not be found"))?
            .collect(),
    };
    if addresses.is_empty() {
        return Err(format!("{host} could not be found"));
    }
    if addresses.iter().any(|a| !is_public(&a.ip())) {
        return Err("a plugin reaches the internet, not this machine or its network".into());
    }
    Ok(addresses[0])
}

fn is_public(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let [a, b, c, _] = v4.octets();
            !(v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_unspecified()
                || v4.is_broadcast()
                || v4.is_multicast()
                || v4.is_documentation()
                // 0.0.0.0/8 "this network"
                || a == 0
                // 100.64.0.0/10, the carrier-grade NAT range.
                || (a == 100 && (b & 0xc0) == 64)
                // 192.0.0.0/24 protocol assignments
                || (a == 192 && b == 0 && c == 0)
                // 198.18.0.0/15 benchmarking
                || (a == 198 && (b & 0xfe) == 18)
                // 240.0.0.0/4 reserved
                || a >= 240)
        }
        IpAddr::V6(v6) => {
            let s = v6.segments();
            let embedded_v4 = |hi: u16, lo: u16| {
                IpAddr::V4(std::net::Ipv4Addr::new(
                    (hi >> 8) as u8,
                    hi as u8,
                    (lo >> 8) as u8,
                    lo as u8,
                ))
            };
            !(v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_multicast()
                // fc00::/7 unique local, fe80::/10 link-local, fec0::/10 site-local
                || (s[0] & 0xfe00) == 0xfc00
                || (s[0] & 0xffc0) == 0xfe80
                || (s[0] & 0xffc0) == 0xfec0
                // ::ffff:a.b.c.d mapped and ::a.b.c.d compatible
                || v6.to_ipv4_mapped().is_some_and(|v4| !is_public(&IpAddr::V4(v4)))
                || (s[..6].iter().all(|x| *x == 0) && !is_public(&embedded_v4(s[6], s[7])))
                // 64:ff9b::/96 NAT64 to a private address
                || (s[0] == 0x64 && s[1] == 0xff9b && !is_public(&embedded_v4(s[6], s[7])))
                // 2002::/16 6to4 around a private address
                || (s[0] == 0x2002 && !is_public(&embedded_v4(s[1], s[2]))))
        }
    }
}

fn error_result(text: &str) -> Value {
    json!({"content": [{"type": "text", "text": text}], "isError": true})
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The tool contract around `body`, which leaves the answer's packed
    /// pointer and length on the stack. `imports` go before everything.
    fn module(imports: &str, memory_pages: u32, body: &str) -> String {
        format!(
            r#"(module
                {imports}
                (memory (export "memory") {memory_pages})
                (data (i32.const 1024) "{{\"content\":[{{\"type\":\"text\",\"text\":\"hi\"}}]}}")
                (func (export "or_alloc") (param i32) (result i32) (i32.const 4096))
                (func (export "or_call") (param i32 i32) (result i64) {body}))"#
        )
    }

    /// The answer at 1024, 41 bytes long.
    const ANSWER: &str = "(i64.or (i64.shl (i64.const 1024) (i64.const 32)) (i64.const 41))";

    fn write(dir: &Path, name: &str, wat: &str) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(
            &path,
            wat::parse_str(wat).expect("the test module assembles"),
        )
        .unwrap();
        path
    }

    fn text(result: &Value) -> String {
        result["content"][0]["text"]
            .as_str()
            .unwrap_or("")
            .to_string()
    }

    #[tokio::test]
    async fn a_tool_answers_through_the_contract() {
        let dir = tempfile::tempdir().unwrap();
        let path = write(dir.path(), "ok.wasm", &module("", 1, ANSWER));
        check(&path, &[]).unwrap();
        let host = WasmHost::default();
        let result = host
            .call(
                "p@local",
                &path,
                &[],
                json!({"tool": "t"}),
                Duration::from_secs(5),
            )
            .await;
        assert_eq!(text(&result), "hi", "{result}");
        assert!(result.get("isError").is_none());
    }

    #[tokio::test]
    async fn a_module_gets_no_ambient_files_environment_or_sockets() {
        let dir = tempfile::tempdir().unwrap();
        // WASI is not offered at all: refused at review, and it can't start.
        let wasi = write(
            dir.path(),
            "wasi.wasm",
            &module(
                r#"(import "wasi_snapshot_preview1" "fd_write" (func (param i32 i32 i32 i32) (result i32)))"#,
                1,
                ANSWER,
            ),
        );
        let err = check(&wasi, &[PluginPower::Network, PluginPower::RunPrograms]).unwrap_err();
        assert!(err.contains("doesn't offer"), "{err}");
        let host = WasmHost::default();
        let all = [
            PluginPower::Network,
            PluginPower::RunPrograms,
            PluginPower::ReadChats,
            PluginPower::AgentTools,
        ];
        let result = host
            .call("p@local", &wasi, &all, json!({}), Duration::from_secs(5))
            .await;
        assert_eq!(result["isError"], true);
        assert!(text(&result).contains("could not start"), "{result}");
        // Nor an environment, by any module name.
        let env = write(
            dir.path(),
            "env.wasm",
            &module(
                r#"(import "env" "getenv" (func (param i32 i32) (result i32)))"#,
                1,
                ANSWER,
            ),
        );
        assert!(check(&env, &all).is_err());
    }

    #[tokio::test]
    async fn the_network_functions_exist_only_with_the_approved_power() {
        let dir = tempfile::tempdir().unwrap();
        let path = write(
            dir.path(),
            "net.wasm",
            &module(
                r#"(import "openremote" "http" (func (param i32 i32) (result i32)))"#,
                1,
                ANSWER,
            ),
        );
        // The manifest didn't ask for the network: refused at review.
        let err = check(&path, &[PluginPower::AgentTools]).unwrap_err();
        assert!(err.contains("network"), "{err}");
        check(&path, &[PluginPower::Network]).unwrap();
        // The install record lacks the power: the import doesn't exist,
        // so the module can't start - even though the file asks for it.
        let host = WasmHost::default();
        let refused = host
            .call(
                "p@local",
                &path,
                &[PluginPower::AgentTools],
                json!({}),
                Duration::from_secs(5),
            )
            .await;
        assert_eq!(refused["isError"], true, "{refused}");
        assert!(text(&refused).contains("could not start"), "{refused}");
        let allowed = host
            .call(
                "p@local",
                &path,
                &[PluginPower::Network],
                json!({}),
                Duration::from_secs(5),
            )
            .await;
        assert_eq!(text(&allowed), "hi", "{allowed}");
    }

    #[tokio::test]
    async fn a_module_that_never_stops_is_stopped_by_its_fuel() {
        let dir = tempfile::tempdir().unwrap();
        let path = write(
            dir.path(),
            "spin.wasm",
            &module("", 1, "(loop (br 0)) (i64.const 0)"),
        );
        let host = WasmHost::with_fuel(5_000_000);
        let started = Instant::now();
        let result = host
            .call("p@local", &path, &[], json!({}), Duration::from_secs(5))
            .await;
        assert_eq!(result["isError"], true);
        assert!(text(&result).contains("time limit"), "{result}");
        assert!(started.elapsed() < Duration::from_secs(30));
        // The default host's fuel is the one the daemon runs with.
        assert_eq!(WasmHost::default().fuel, FUEL);
    }

    #[tokio::test]
    async fn a_module_cannot_take_more_memory_than_its_limit() {
        let dir = tempfile::tempdir().unwrap();
        // Asking for 2000 pages (125 MiB) up front: it can't start.
        let big = write(dir.path(), "big.wasm", &module("", 2000, ANSWER));
        let host = WasmHost::default();
        let result = host
            .call("p@local", &big, &[], json!({}), Duration::from_secs(5))
            .await;
        assert_eq!(result["isError"], true, "{result}");
        // Growing past the limit fails: memory.grow answers -1, and this
        // module turns that into a trap.
        let grow = write(
            dir.path(),
            "grow.wasm",
            &module(
                "",
                1,
                &format!(
                    "(if (i32.eq (memory.grow (i32.const 2000)) (i32.const -1)) (then unreachable)) {ANSWER}"
                ),
            ),
        );
        let result = host
            .call("p@local", &grow, &[], json!({}), Duration::from_secs(5))
            .await;
        assert_eq!(result["isError"], true, "{result}");
        // Within the limit it grows and answers.
        let small = write(
            dir.path(),
            "small.wasm",
            &module(
                "",
                1,
                &format!("(drop (memory.grow (i32.const 16))) {ANSWER}"),
            ),
        );
        let result = host
            .call("p@local", &small, &[], json!({}), Duration::from_secs(5))
            .await;
        assert_eq!(text(&result), "hi", "{result}");
    }

    #[tokio::test]
    async fn a_request_reaches_the_internet_only() {
        for url in [
            "http://127.0.0.1/",
            "http://localhost:8080/",
            "http://10.0.0.1/",
            "http://192.168.1.1/",
            "http://169.254.169.254/latest/meta-data/",
            "http://[::1]/",
            "http://[::ffff:127.0.0.1]/",
            "http://[fd00::1]/",
            "http://0.0.0.0/",
            "http://100.64.0.1/",
            "file:///etc/passwd",
            "ftp://example.com/",
        ] {
            assert!(public_address(url).await.is_err(), "{url}");
        }
        assert_eq!(
            public_address("https://1.1.1.1/x").await.unwrap(),
            "1.1.1.1:443".parse().unwrap()
        );
    }

    #[tokio::test]
    async fn a_lookup_or_request_that_never_finishes_ends_at_its_limit() {
        let started = Instant::now();
        let never = within::<()>(
            Duration::from_millis(200),
            std::future::pending::<Result<(), String>>(),
        )
        .await;
        assert!(never.unwrap_err().contains("took longer"));
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[tokio::test]
    async fn a_call_waits_for_a_turn_no_longer_than_its_time() {
        let dir = tempfile::tempdir().unwrap();
        let path = write(dir.path(), "ok.wasm", &module("", 1, ANSWER));
        // Every turn is taken (no permits): the call gives up at its time.
        let host = WasmHost::with_limits(FUEL, 0);
        let started = Instant::now();
        let result = host
            .call("p@local", &path, &[], json!({}), Duration::from_millis(200))
            .await;
        assert_eq!(result["isError"], true);
        assert!(
            text(&result).contains("waited past its time limit"),
            "{result}"
        );
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn a_request_past_the_calls_deadline_or_count_is_refused() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let mut host = Host {
            limits: StoreLimitsBuilder::new().build(),
            plugin: "p".into(),
            requests_left: 1,
            deadline: Instant::now(),
            held: Vec::new(),
            http: openremote_cloud::https::Http::new(),
            runtime: runtime.handle().clone(),
        };
        let answer = http(&mut host, br#"{"url": "https://1.1.1.1/"}"#);
        assert!(answer["error"].as_str().unwrap().contains("time is up"));
        host.deadline = Instant::now() + Duration::from_secs(60);
        host.requests_left = 0;
        let answer = http(&mut host, br#"{"url": "https://1.1.1.1/"}"#);
        assert!(answer["error"].as_str().unwrap().contains("at most"));
        // A private address is refused before anything connects.
        host.requests_left = 1;
        let answer = http(&mut host, br#"{"url": "http://127.0.0.1:1/"}"#);
        assert!(
            answer["error"]
                .as_str()
                .unwrap()
                .contains("not this machine")
        );
    }
}
