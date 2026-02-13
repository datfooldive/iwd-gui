use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use zbus::DBusError;
use zbus::blocking::{Connection, Proxy};
use zvariant::{ObjectPath, OwnedObjectPath, OwnedValue};

use crate::models::{DeviceInfo, KnownNetwork, VisibleNetwork};

const IWD_SERVICE: &str = "net.connman.iwd";
const OBJECT_MANAGER_IFACE: &str = "org.freedesktop.DBus.ObjectManager";
const DEVICE_IFACE: &str = "net.connman.iwd.Device";
const STATION_IFACE: &str = "net.connman.iwd.Station";
const NETWORK_IFACE: &str = "net.connman.iwd.Network";
const KNOWN_NETWORK_IFACE: &str = "net.connman.iwd.KnownNetwork";
const AGENT_MANAGER_IFACE: &str = "net.connman.iwd.AgentManager";
const AGENT_OBJECT_PATH: &str = "/com/github/datfooldive/iwd_gui/agent";

type PropMap = HashMap<String, OwnedValue>;
type InterfaceMap = HashMap<String, PropMap>;
type ManagedObjects = HashMap<OwnedObjectPath, InterfaceMap>;

#[derive(Debug)]
pub(crate) struct IwdDbus {
    conn: Connection,
}

#[derive(Debug, Default)]
struct AgentState {
    passphrase: String,
}

#[derive(Debug)]
struct IwdAgent {
    state: Arc<Mutex<AgentState>>,
}

impl IwdAgent {
    fn new(passphrase: String) -> Self {
        Self {
            state: Arc::new(Mutex::new(AgentState { passphrase })),
        }
    }

    fn passphrase_or_cancel(&self) -> Result<String, AgentError> {
        let state = self
            .state
            .lock()
            .map_err(|_| AgentError::Failed("agent lock poisoned".to_string()))?;
        if state.passphrase.trim().is_empty() {
            Err(AgentError::Canceled("passphrase is empty".to_string()))
        } else {
            Ok(state.passphrase.clone())
        }
    }
}

#[derive(Debug, DBusError)]
#[zbus(prefix = "net.connman.iwd.Error")]
enum AgentError {
    Canceled(String),
    Failed(String),
    #[zbus(error)]
    ZBus(zbus::Error),
}

#[zbus::interface(name = "net.connman.iwd.Agent")]
impl IwdAgent {
    fn release(&self) {}

    fn cancel(&self, _reason: &str) {}

    fn request_passphrase(&self, _network: OwnedObjectPath) -> Result<String, AgentError> {
        self.passphrase_or_cancel()
    }

    fn request_private_key_passphrase(&self, _path: &str) -> Result<String, AgentError> {
        self.passphrase_or_cancel()
    }

    fn request_user_name_and_password(
        &self,
        _name: &str,
        _service: &str,
    ) -> Result<(String, String), AgentError> {
        let pass = self.passphrase_or_cancel()?;
        Ok(("".to_string(), pass))
    }

    fn request_user_password(&self, _name: &str, _service: &str) -> Result<String, AgentError> {
        self.passphrase_or_cancel()
    }
}

struct RegisteredAgent<'a> {
    conn: &'a Connection,
}

impl<'a> RegisteredAgent<'a> {
    fn new(conn: &'a Connection, passphrase: &str) -> Result<Self, String> {
        let object_server = conn.object_server();
        let _ = object_server.remove::<IwdAgent, _>(AGENT_OBJECT_PATH);
        object_server
            .at(AGENT_OBJECT_PATH, IwdAgent::new(passphrase.to_string()))
            .map_err(|e| e.to_string())?;

        let manager = Proxy::new(conn, IWD_SERVICE, "/net/connman/iwd", AGENT_MANAGER_IFACE)
            .map_err(|e| e.to_string())?;
        let path = ObjectPath::try_from(AGENT_OBJECT_PATH)
            .map_err(|e| format!("invalid agent path: {e}"))?;
        let _: () = manager
            .call("RegisterAgent", &(path))
            .map_err(|e| e.to_string())?;

        Ok(Self { conn })
    }
}

impl Drop for RegisteredAgent<'_> {
    fn drop(&mut self) {
        if let Ok(manager) = Proxy::new(
            self.conn,
            IWD_SERVICE,
            "/net/connman/iwd",
            AGENT_MANAGER_IFACE,
        ) {
            if let Ok(path) = ObjectPath::try_from(AGENT_OBJECT_PATH) {
                let _ = manager.call::<_, _, ()>("UnregisterAgent", &(path));
            }
        }
        let _ = self
            .conn
            .object_server()
            .remove::<IwdAgent, _>(AGENT_OBJECT_PATH);
    }
}

impl IwdDbus {
    pub(crate) fn new() -> Result<Self, String> {
        let conn = Connection::system().map_err(|e| e.to_string())?;
        Ok(Self { conn })
    }

    fn managed_objects(&self) -> Result<ManagedObjects, String> {
        let proxy = Proxy::new(&self.conn, IWD_SERVICE, "/", OBJECT_MANAGER_IFACE)
            .map_err(|e| e.to_string())?;
        proxy
            .call("GetManagedObjects", &())
            .map_err(|e| e.to_string())
    }

    pub(crate) fn list_devices(&self) -> Result<Vec<DeviceInfo>, String> {
        let objects = self.managed_objects()?;
        let mut out = Vec::new();

        for (path, interfaces) in objects {
            if !interfaces.contains_key(DEVICE_IFACE) {
                continue;
            }

            let path_str = path.as_str().to_string();
            let proxy = Proxy::new(&self.conn, IWD_SERVICE, path.as_str(), DEVICE_IFACE)
                .map_err(|e| e.to_string())?;
            let name: String = proxy
                .get_property("Name")
                .map_err(|e| format!("Failed to read device name at {path_str}: {e}"))?;
            out.push(DeviceInfo {
                name,
                path: path_str,
            });
        }

        out.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(out)
    }

    pub(crate) fn list_visible_networks(
        &self,
        selected_device_path: Option<&str>,
    ) -> Result<Vec<VisibleNetwork>, String> {
        let objects = self.managed_objects()?;
        let mut out = Vec::new();

        for (path, interfaces) in objects {
            if !interfaces.contains_key(NETWORK_IFACE) {
                continue;
            }

            let path_str = path.as_str().to_string();
            let proxy = Proxy::new(&self.conn, IWD_SERVICE, path.as_str(), NETWORK_IFACE)
                .map_err(|e| e.to_string())?;

            let ssid: String = proxy
                .get_property("Name")
                .map_err(|e| format!("Failed to read network name at {}: {e}", path.as_str()))?;
            let security: String = proxy
                .get_property("Type")
                .unwrap_or_else(|_| "-".to_string());
            let connected: bool = proxy.get_property("Connected").unwrap_or(false);
            let signal_dbm: i16 = proxy.get_property("Signal").unwrap_or(0);
            let signal = if signal_dbm == 0 {
                "-".to_string()
            } else {
                format!("{signal_dbm} dBm")
            };

            let device_path: Option<String> = proxy
                .get_property::<OwnedObjectPath>("Device")
                .ok()
                .map(|v| v.as_str().to_string());

            if let Some(sel) = selected_device_path {
                if let Some(dev) = device_path.as_deref() {
                    if dev != sel {
                        continue;
                    }
                }
            }

            out.push(VisibleNetwork {
                ssid,
                security,
                signal,
                connected,
                path: path_str,
                device_path,
            });
        }

        out.sort_by(|a, b| a.ssid.cmp(&b.ssid));
        Ok(out)
    }

    pub(crate) fn list_known_networks(&self) -> Result<Vec<KnownNetwork>, String> {
        let objects = self.managed_objects()?;
        let mut out = Vec::new();

        for (path, interfaces) in objects {
            if !interfaces.contains_key(KNOWN_NETWORK_IFACE) {
                continue;
            }

            let proxy = Proxy::new(&self.conn, IWD_SERVICE, path.as_str(), KNOWN_NETWORK_IFACE)
                .map_err(|e| e.to_string())?;

            let name: String = proxy
                .get_property("Name")
                .map_err(|e| format!("Failed to read known network name: {e}"))?;
            let network_type: String = proxy
                .get_property("Type")
                .unwrap_or_else(|_| "-".to_string());
            let autoconnect: Option<bool> = proxy.get_property("AutoConnect").ok();
            let hidden: Option<bool> = proxy.get_property("Hidden").ok();

            out.push(KnownNetwork {
                name,
                network_type,
                autoconnect,
                hidden,
                path: path.as_str().to_string(),
            });
        }

        out.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(out)
    }

    pub(crate) fn scan(&self, device_path: &str) -> Result<(), String> {
        let proxy = Proxy::new(&self.conn, IWD_SERVICE, device_path, STATION_IFACE)
            .map_err(|e| e.to_string())?;
        let _: () = proxy.call("Scan", &()).map_err(|e| e.to_string())?;
        Ok(())
    }

    pub(crate) fn connect_network(
        &self,
        network_path: &str,
        passphrase: Option<&str>,
    ) -> Result<(), String> {
        let _agent = if let Some(passphrase) = passphrase {
            Some(RegisteredAgent::new(&self.conn, passphrase)?)
        } else {
            None
        };
        let proxy = Proxy::new(&self.conn, IWD_SERVICE, network_path, NETWORK_IFACE)
            .map_err(|e| e.to_string())?;
        let _: () = proxy.call("Connect", &()).map_err(|e| e.to_string())?;
        Ok(())
    }

    pub(crate) fn forget_known_network(&self, known_path: &str) -> Result<(), String> {
        let proxy = Proxy::new(&self.conn, IWD_SERVICE, known_path, KNOWN_NETWORK_IFACE)
            .map_err(|e| e.to_string())?;
        let _: () = proxy.call("Forget", &()).map_err(|e| e.to_string())?;
        Ok(())
    }

    pub(crate) fn set_known_autoconnect(
        &self,
        known_path: &str,
        enabled: bool,
    ) -> Result<(), String> {
        let proxy = Proxy::new(&self.conn, IWD_SERVICE, known_path, KNOWN_NETWORK_IFACE)
            .map_err(|e| e.to_string())?;
        proxy
            .set_property("AutoConnect", enabled)
            .map_err(|e| e.to_string())
    }
}
