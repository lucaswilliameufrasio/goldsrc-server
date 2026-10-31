//! Opt-in Linux diagnostic adapter. No SDK code, tickets, or credentials on disk.
use libloading::Library;
use std::collections::HashMap;
use std::ffi::{c_char, c_void};
use std::io;
use std::net::SocketAddr;
use std::time::{Duration, Instant};

type Server = *mut c_void;
type Void = unsafe extern "C" fn(Server);
type Flag = unsafe extern "C" fn(Server, bool);
type Text = unsafe extern "C" fn(Server, *const c_char);
type Logged = unsafe extern "C" fn(Server) -> bool;
type Identity = unsafe extern "C" fn(Server) -> u64;
type Authenticate = unsafe extern "C" fn(Server, u32, *const c_void, u32, *mut u64) -> bool;
type Disconnect = unsafe extern "C" fn(Server, u64);
type RunFrame = unsafe extern "C" fn(i32);
type NextCallback = unsafe extern "C" fn(i32, *mut Callback) -> bool;
type Shutdown = unsafe extern "C" fn();

#[repr(C, packed(4))]
struct Callback {
    user: i32,
    id: i32,
    data: *const u8,
    size: i32,
}

struct Pending {
    peer: SocketAddr,
    started: Instant,
}

const AUTH_TIMEOUT: Duration = Duration::from_secs(20);

fn auth_outcome(callback: i32, size: i32, ready: bool, age: Duration) -> &'static str {
    if age >= AUTH_TIMEOUT {
        "steam_auth_timeout"
    } else if callback == 201 && size >= 16 && ready {
        "steam_auth_approved"
    } else {
        "steam_auth_denied"
    }
}

pub struct Event {
    pub peer: SocketAddr,
    pub outcome: &'static str,
    pub reason: &'static str,
    pub result_code: Option<i32>,
}

pub struct Steam {
    // Function pointers never outlive this library; keep it until after SDK shutdown.
    _library: Library,
    server: Server,
    pipe: i32,
    advertise: Flag,
    logoff: Void,
    shutdown: Shutdown,
    logged: Logged,
    identity: Identity,
    authenticate: Authenticate,
    disconnect: Disconnect,
    run_frame: RunFrame,
    next_callback: NextCallback,
    free_callback: RunFrame,
    pending: HashMap<u64, Pending>,
    active: HashMap<u64, SocketAddr>,
    ready: bool,
    started: Instant,
}

// SAFETY: callers select known SDK flat-C signatures from inspected 1.65 headers.
unsafe fn symbol<T: Copy>(library: &Library, name: &[u8]) -> io::Result<T> {
    unsafe { library.get::<T>(name) }
        .map(|value| *value)
        .map_err(|_| io::Error::other("Required Steam SDK symbol unavailable"))
}

impl Steam {
    pub fn from_environment() -> io::Result<Option<Self>> {
        let Some(path) = std::env::var_os("GOLDSRC_STEAM_API") else {
            return Ok(None);
        };
        if !cfg!(all(target_os = "linux", target_pointer_width = "64")) {
            return Err(io::Error::other("Steam diagnostic requires 64-bit Linux"));
        }
        let versions = std::fs::read(
            std::env::var_os("GOLDSRC_STEAM_INTERFACES")
                .ok_or_else(|| io::Error::other("Use the isolated Steam probe launcher"))?,
        )?;
        if versions.len() > 4096 || !versions.ends_with(&[0, 0]) {
            return Err(io::Error::other("Invalid Steam interface negotiation list"));
        }
        // SAFETY: opt-in path points to the owner's local, trusted native SDK.
        unsafe {
            let library =
                Library::new(path).map_err(|_| io::Error::other("Could not load Steam SDK"))?;
            let init: unsafe extern "C" fn(
                u32,
                u16,
                u16,
                i32,
                *const c_char,
                *const c_char,
                *mut c_char,
            ) -> i32 = symbol(&library, b"SteamInternal_GameServer_Init_V2\0")?;
            let accessor: unsafe extern "C" fn() -> Server =
                symbol(&library, b"SteamAPI_SteamGameServer_v015\0")?;
            let shutdown: Shutdown = symbol(&library, b"SteamGameServer_Shutdown\0")?;
            let advertise: Flag = symbol(
                &library,
                b"SteamAPI_ISteamGameServer_SetAdvertiseServerActive\0",
            )?;
            let logon: Void = symbol(&library, b"SteamAPI_ISteamGameServer_LogOnAnonymous\0")?;
            let logoff: Void = symbol(&library, b"SteamAPI_ISteamGameServer_LogOff\0")?;
            let product: Text = symbol(&library, b"SteamAPI_ISteamGameServer_SetProduct\0")?;
            let description: Text =
                symbol(&library, b"SteamAPI_ISteamGameServer_SetGameDescription\0")?;
            let mod_dir: Text = symbol(&library, b"SteamAPI_ISteamGameServer_SetModDir\0")?;
            let dedicated: Flag =
                symbol(&library, b"SteamAPI_ISteamGameServer_SetDedicatedServer\0")?;
            let logged = symbol(&library, b"SteamAPI_ISteamGameServer_BLoggedOn\0")?;
            let identity = symbol(&library, b"SteamAPI_ISteamGameServer_GetSteamID\0")?;
            let authenticate = symbol(
                &library,
                b"SteamAPI_ISteamGameServer_SendUserConnectAndAuthenticate_DEPRECATED\0",
            )?;
            let disconnect = symbol(
                &library,
                b"SteamAPI_ISteamGameServer_SendUserDisconnect_DEPRECATED\0",
            )?;
            let dispatch_init: Shutdown = symbol(&library, b"SteamAPI_ManualDispatch_Init\0")?;
            let pipe_handle: unsafe extern "C" fn() -> i32 =
                symbol(&library, b"SteamGameServer_GetHSteamPipe\0")?;
            let run_frame = symbol(&library, b"SteamAPI_ManualDispatch_RunFrame\0")?;
            let next_callback = symbol(&library, b"SteamAPI_ManualDispatch_GetNextCallback\0")?;
            let free_callback = symbol(&library, b"SteamAPI_ManualDispatch_FreeLastCallback\0")?;
            let mut error = [0 as c_char; 1024];
            // Shared-query mode creates no SDK query listener. Never forward master packets.
            let result = init(
                0x7f000001,
                27035,
                0xffff,
                2,
                c"1.1.2.7".as_ptr(),
                versions.as_ptr().cast(),
                error.as_mut_ptr(),
            );
            if result != 0 {
                return Err(io::Error::other(format!(
                    "Steam initialization failed, code {result}"
                )));
            }
            let server = accessor();
            if server.is_null() {
                shutdown();
                return Err(io::Error::other("Steam server interface unavailable"));
            }
            advertise(server, false);
            product(server, c"cstrike".as_ptr());
            description(server, c"Local Rust authentication diagnostic".as_ptr());
            mod_dir(server, c"cstrike".as_ptr());
            dedicated(server, true);
            dispatch_init();
            let pipe = pipe_handle();
            if pipe == 0 {
                shutdown();
                return Err(io::Error::other("Steam callback pipe unavailable"));
            }
            logon(server);
            Ok(Some(Self {
                _library: library,
                server,
                pipe,
                advertise,
                logoff,
                shutdown,
                logged,
                identity,
                authenticate,
                disconnect,
                run_frame,
                next_callback,
                free_callback,
                pending: HashMap::new(),
                active: HashMap::new(),
                ready: false,
                started: Instant::now(),
            }))
        }
    }

    pub fn server_identity(&self) -> Option<u64> {
        if self.ready {
            // SAFETY: live SDK interface owned by self, accessed on its single thread.
            let id = unsafe { (self.identity)(self.server) };
            if id != 0 {
                return Some(id);
            }
        }
        None
    }

    pub fn submit(&mut self, peer: SocketAddr, ticket: &[u8], now: Instant) -> &'static str {
        if self.server_identity().is_none() {
            return "steam_not_ready";
        }
        if self.pending.len() + self.active.len() >= 32
            || self.pending.values().any(|entry| entry.peer == peer)
            || self.active.values().any(|address| *address == peer)
        {
            return "steam_auth_capacity";
        }
        let std::net::IpAddr::V4(ip) = peer.ip() else {
            return "steam_auth_ipv4_required";
        };
        if ticket.is_empty() || ticket.len() >= 1024 {
            return "invalid_ticket_length";
        }
        let mut id = 0;
        // SAFETY: borrowed ticket is valid throughout synchronous FFI call. Only
        // numeric identity is retained; no ticket copy/log is made by this adapter.
        let accepted = unsafe {
            (self.authenticate)(
                self.server,
                u32::from(ip),
                ticket.as_ptr().cast(),
                ticket.len() as u32,
                &mut id,
            )
        };
        if !accepted || id == 0 {
            return "steam_auth_rejected_basic";
        }
        if self.pending.contains_key(&id) || self.active.contains_key(&id) {
            return "steam_auth_duplicate_identity";
        }
        self.pending.insert(id, Pending { peer, started: now });
        "steam_auth_pending"
    }

    pub fn close_peer(&mut self, peer: SocketAddr) {
        if let Some(id) = self
            .active
            .iter()
            .find_map(|(id, address)| if *address == peer { Some(*id) } else { None })
        {
            self.active.remove(&id);
            // SAFETY: this identity belongs to an active session owned by this runtime.
            unsafe { (self.disconnect)(self.server, id) };
        }
    }

    pub fn poll(&mut self, now: Instant) -> io::Result<Vec<Event>> {
        let mut events = Vec::new();
        let mut callback = Callback {
            user: 0,
            id: 0,
            data: std::ptr::null(),
            size: 0,
        };
        let mut backend_failed = false;
        // SAFETY: callback buffer/layout matches SDK Linux pack(4), callbacks
        // are inspected only until FreeLastCallback on this same thread.
        unsafe {
            (self.run_frame)(self.pipe);
            for _ in 0..256 {
                if !(self.next_callback)(self.pipe, &mut callback) {
                    break;
                }
                match callback.id {
                    101 => {
                        self.ready =
                            (self.logged)(self.server) && (self.identity)(self.server) != 0;
                        (self.advertise)(self.server, false);
                        eprintln!("Steam diagnostic: server_connected, ready={}", self.ready);
                    }
                    102 | 103 => {
                        self.ready = false;
                        backend_failed = true;
                    }
                    201..=203 if callback.size >= 8 && !callback.data.is_null() => {
                        let id = std::ptr::read_unaligned(callback.data.cast::<u64>());
                        if let Some(entry) = self.pending.remove(&id) {
                            let outcome = auth_outcome(
                                callback.id,
                                callback.size,
                                self.ready && (self.logged)(self.server),
                                now.saturating_duration_since(entry.started),
                            );
                            events.push(Event {
                                peer: entry.peer,
                                outcome,
                                result_code: if callback.id != 201 && callback.size >= 12 {
                                    Some(std::ptr::read_unaligned(callback.data.add(8).cast::<i32>()))
                                } else {
                                    None
                                },
                                reason: if outcome == "steam_auth_approved" {
                                    "Steam authenticated this player; development PoC sign-on is not implemented."
                                } else {
                                    "Steam authentication denied."
                                },
                            });
                            if outcome == "steam_auth_approved" {
                                self.active.insert(id, entry.peer);
                            } else {
                                (self.disconnect)(self.server, id);
                            }
                        } else if callback.id != 201
                            && let Some(peer) = self.active.remove(&id)
                        {
                            events.push(Event {
                                peer,
                                outcome: "steam_auth_revoked",
                                reason: "Steam authentication revoked.",
                                result_code: None,
                            });
                            (self.disconnect)(self.server, id);
                        }
                    }
                    _ => {}
                }
                (self.free_callback)(self.pipe);
            }
        }
        let expired: Vec<u64> = self
            .pending
            .iter()
            .filter_map(|(id, entry)| {
                if backend_failed || now.saturating_duration_since(entry.started) >= AUTH_TIMEOUT {
                    Some(*id)
                } else {
                    None
                }
            })
            .collect();
        for id in expired {
            if let Some(entry) = self.pending.remove(&id) {
                events.push(Event {
                    peer: entry.peer,
                    outcome: "steam_auth_timeout",
                    reason: "Steam authentication timed out or backend disconnected.",
                    result_code: None,
                });
                // SAFETY: numeric ID belongs to an auth request owned by this instance.
                unsafe { (self.disconnect)(self.server, id) };
            }
        }
        if backend_failed
            || (!self.ready
                && now.saturating_duration_since(self.started) >= Duration::from_secs(30))
        {
            return Err(io::Error::other(
                "Steam backend unavailable or server logon timed out",
            ));
        }
        Ok(events)
    }
}

impl Drop for Steam {
    fn drop(&mut self) {
        // SAFETY: SDK remains loaded until this Drop completes. No callbacks or
        // function pointers escape this instance. End all owned pending sessions.
        unsafe {
            for id in self.pending.keys() {
                (self.disconnect)(self.server, *id);
            }
            for id in self.active.keys() {
                (self.disconnect)(self.server, *id);
            }
            (self.advertise)(self.server, false);
            (self.logoff)(self.server);
            (self.shutdown)();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Callback;

    #[test]
    fn linux_callback_abi_matches_inspected_sdk() {
        assert_eq!(std::mem::size_of::<Callback>(), 20);
        assert_eq!(std::mem::offset_of!(Callback, data), 8);
        assert_eq!(std::mem::offset_of!(Callback, size), 16);
    }

    #[test]
    fn approval_requires_ready_backend_valid_callback_and_unexpired_request() {
        use super::{AUTH_TIMEOUT, auth_outcome};
        use std::time::Duration;
        assert_eq!(
            auth_outcome(201, 16, true, Duration::ZERO),
            "steam_auth_approved"
        );
        assert_eq!(
            auth_outcome(201, 8, true, Duration::ZERO),
            "steam_auth_denied"
        );
        assert_eq!(
            auth_outcome(201, 16, false, Duration::ZERO),
            "steam_auth_denied"
        );
        assert_eq!(
            auth_outcome(202, 140, true, Duration::ZERO),
            "steam_auth_denied"
        );
        assert_eq!(
            auth_outcome(203, 12, true, Duration::ZERO),
            "steam_auth_denied"
        );
        assert_eq!(
            auth_outcome(201, 16, true, AUTH_TIMEOUT),
            "steam_auth_timeout"
        );
    }
}
