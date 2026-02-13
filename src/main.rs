use eframe::egui;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use zbus::DBusError;
use zbus::blocking::{Connection, Proxy};
use zvariant::{ObjectPath, OwnedObjectPath, OwnedValue};

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

#[derive(Clone, Debug, Default)]
struct DeviceInfo {
    name: String,
    path: String,
}

#[derive(Clone, Debug, Default)]
struct VisibleNetwork {
    ssid: String,
    security: String,
    signal: String,
    connected: bool,
    path: String,
    device_path: Option<String>,
}

#[derive(Clone, Debug, Default)]
struct KnownNetwork {
    name: String,
    network_type: String,
    autoconnect: Option<bool>,
    hidden: Option<bool>,
    path: String,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum ActiveTab {
    #[default]
    Networks,
    Saved,
}

#[derive(Debug)]
struct IwdGuiApp {
    initialized: bool,
    active_tab: ActiveTab,
    devices: Vec<DeviceInfo>,
    selected_device_path: Option<String>,
    visible_networks: Vec<VisibleNetwork>,
    known_networks: Vec<KnownNetwork>,
    connect_ssid: String,
    connect_passphrase: String,
    selected_known_path: Option<String>,
    selected_known_details: String,
    selected_known_autoconnect: Option<bool>,
    status_line: String,
}

impl Default for IwdGuiApp {
    fn default() -> Self {
        Self {
            initialized: false,
            active_tab: ActiveTab::Networks,
            devices: Vec::new(),
            selected_device_path: None,
            visible_networks: Vec::new(),
            known_networks: Vec::new(),
            connect_ssid: String::new(),
            connect_passphrase: String::new(),
            selected_known_path: None,
            selected_known_details: String::new(),
            selected_known_autoconnect: None,
            status_line: "Ready".to_string(),
        }
    }
}

impl IwdGuiApp {
    fn set_status(&mut self, status: impl Into<String>) {
        self.status_line = status.into();
    }

    fn selected_device_name(&self) -> String {
        self.devices
            .iter()
            .find(|d| Some(d.path.as_str()) == self.selected_device_path.as_deref())
            .map(|d| d.name.clone())
            .unwrap_or_else(|| "(none)".to_string())
    }

    fn refresh_all(&mut self) {
        let backend = match IwdDbus::new() {
            Ok(v) => v,
            Err(err) => {
                self.set_status(format!("Failed to connect to iwd D-Bus: {err}"));
                return;
            }
        };

        match backend.list_devices() {
            Ok(devices) => {
                self.devices = devices;
                if self.devices.is_empty() {
                    self.selected_device_path = None;
                    self.set_status("No wireless devices found");
                } else if self
                    .devices
                    .iter()
                    .all(|d| Some(d.path.as_str()) != self.selected_device_path.as_deref())
                {
                    self.selected_device_path = Some(self.devices[0].path.clone());
                }
            }
            Err(err) => {
                self.set_status(format!("Failed to list devices: {err}"));
                return;
            }
        }

        let selected_device = self.selected_device_path.clone();

        match backend.list_visible_networks(selected_device.as_deref()) {
            Ok(networks) => {
                self.visible_networks = networks;
            }
            Err(err) => {
                self.set_status(format!("Failed to load visible networks: {err}"));
                return;
            }
        }

        match backend.list_known_networks() {
            Ok(known) => {
                self.known_networks = known;
            }
            Err(err) => {
                self.set_status(format!("Failed to load saved networks: {err}"));
                return;
            }
        }

        if let Some(path) = self.selected_known_path.clone() {
            if let Some(found) = self.known_networks.iter().find(|k| k.path == path) {
                self.selected_known_details = format_known_network(found);
                self.selected_known_autoconnect = found.autoconnect;
            } else {
                self.selected_known_path = None;
                self.selected_known_details.clear();
                self.selected_known_autoconnect = None;
            }
        }

        self.set_status(format!(
            "Loaded {} device(s), {} visible network(s), {} saved network(s)",
            self.devices.len(),
            self.visible_networks.len(),
            self.known_networks.len()
        ));
    }

    fn scan_networks(&mut self) {
        let Some(device_path) = self.selected_device_path.clone() else {
            self.set_status("Select a device first");
            return;
        };

        let backend = match IwdDbus::new() {
            Ok(v) => v,
            Err(err) => {
                self.set_status(format!("Failed to connect to iwd D-Bus: {err}"));
                return;
            }
        };

        match backend.scan(&device_path) {
            Ok(_) => {
                self.set_status("Scan requested");
                self.refresh_all();
            }
            Err(err) => self.set_status(format!("Scan failed: {err}")),
        }
    }

    fn connect_to_selected_network(&mut self) {
        let ssid = self.connect_ssid.trim().to_string();
        if ssid.is_empty() {
            self.set_status("SSID cannot be empty");
            return;
        }

        let selected_device = self.selected_device_path.clone();
        let candidate = self
            .visible_networks
            .iter()
            .find(|n| {
                n.ssid == ssid
                    && (selected_device.is_none()
                        || n.device_path.as_deref() == selected_device.as_deref())
            })
            .cloned();

        let Some(network) = candidate else {
            self.set_status("Selected SSID not found in visible list");
            return;
        };

        let backend = match IwdDbus::new() {
            Ok(v) => v,
            Err(err) => {
                self.set_status(format!("Failed to connect to iwd D-Bus: {err}"));
                return;
            }
        };

        let passphrase = self.connect_passphrase.trim();
        let passphrase = if passphrase.is_empty() {
            None
        } else {
            Some(passphrase)
        };

        match backend.connect_network(&network.path, passphrase) {
            Ok(_) => {
                self.set_status(format!("Connect requested for `{}`", network.ssid));
                self.refresh_all();
            }
            Err(err) => self.set_status(format!("Connection failed: {err}")),
        }
    }

    fn forget_known_network(&mut self, known_path: &str, name: &str) {
        let backend = match IwdDbus::new() {
            Ok(v) => v,
            Err(err) => {
                self.set_status(format!("Failed to connect to iwd D-Bus: {err}"));
                return;
            }
        };

        match backend.forget_known_network(known_path) {
            Ok(_) => {
                if self.selected_known_path.as_deref() == Some(known_path) {
                    self.selected_known_path = None;
                    self.selected_known_details.clear();
                    self.selected_known_autoconnect = None;
                }
                self.set_status(format!("Forgot saved network `{name}`"));
                self.refresh_all();
            }
            Err(err) => self.set_status(format!("Failed to forget `{name}`: {err}")),
        }
    }

    fn select_known_network(&mut self, known: &KnownNetwork) {
        self.selected_known_path = Some(known.path.clone());
        self.selected_known_autoconnect = known.autoconnect;
        self.selected_known_details = format_known_network(known);
        self.set_status(format!("Loaded saved network details for `{}`", known.name));
    }

    fn set_known_autoconnect(&mut self, enabled: bool) {
        let Some(path) = self.selected_known_path.clone() else {
            self.set_status("Select a saved network first");
            return;
        };

        let backend = match IwdDbus::new() {
            Ok(v) => v,
            Err(err) => {
                self.set_status(format!("Failed to connect to iwd D-Bus: {err}"));
                return;
            }
        };

        match backend.set_known_autoconnect(&path, enabled) {
            Ok(_) => {
                self.set_status("Updated AutoConnect");
                self.refresh_all();
            }
            Err(err) => self.set_status(format!("Failed to update AutoConnect: {err}")),
        }
    }

    fn draw_networks_tab(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if ui.button("Scan").clicked() {
                self.scan_networks();
            }
            if ui.button("Refresh").clicked() {
                self.refresh_all();
            }
        });

        ui.separator();
        ui.label("Connect");
        ui.horizontal(|ui| {
            ui.label("SSID");
            ui.text_edit_singleline(&mut self.connect_ssid);
            ui.label("Passphrase");
            ui.add(egui::TextEdit::singleline(&mut self.connect_passphrase).password(true));
            if ui.button("Connect").clicked() {
                self.connect_to_selected_network();
            }
        });

        ui.separator();
        ui.label("Visible Networks");
        egui::ScrollArea::vertical().show(ui, |ui| {
            egui::Grid::new("visible_networks_grid")
                .num_columns(6)
                .striped(true)
                .show(ui, |ui| {
                    ui.strong("SSID");
                    ui.strong("Security");
                    ui.strong("Signal");
                    ui.strong("Connected");
                    ui.strong("Action");
                    ui.end_row();

                    let selected_device = self.selected_device_path.clone();
                    let networks = self.visible_networks.clone();
                    for network in networks {
                        if selected_device.is_some()
                            && network.device_path.as_deref() != selected_device.as_deref()
                        {
                            continue;
                        }

                        let is_selected = self.connect_ssid == network.ssid;
                        if ui.selectable_label(is_selected, &network.ssid).clicked() {
                            self.connect_ssid = network.ssid.clone();
                        }
                        ui.label(network.security);
                        ui.label(network.signal);
                        ui.label(if network.connected { "yes" } else { "no" });
                        if ui.button("Connect").clicked() {
                            self.connect_ssid = network.ssid;
                            self.connect_to_selected_network();
                        }
                        ui.end_row();
                    }
                });
        });
    }

    fn draw_saved_tab(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if ui.button("Refresh").clicked() {
                self.refresh_all();
            }
        });

        ui.separator();
        ui.label("Saved Networks");

        egui::ScrollArea::vertical()
            .max_height(220.0)
            .show(ui, |ui| {
                egui::Grid::new("known_networks_grid")
                    .num_columns(4)
                    .striped(true)
                    .show(ui, |ui| {
                        ui.strong("Name");
                        ui.strong("Type");
                        ui.strong("Action");
                        ui.end_row();

                        let known = self.known_networks.clone();
                        for network in known {
                            let is_selected =
                                self.selected_known_path.as_deref() == Some(network.path.as_str());
                            if ui.selectable_label(is_selected, &network.name).clicked() {
                                self.select_known_network(&network);
                            }
                            ui.label(network.network_type.clone());
                            if ui.button("Forget").clicked() {
                                self.forget_known_network(&network.path, &network.name);
                            }
                            ui.end_row();
                        }
                    });
            });

        if self.selected_known_path.is_some() {
            ui.separator();
            if let Some(autoconnect) = self.selected_known_autoconnect {
                let mut value = autoconnect;
                if ui.checkbox(&mut value, "AutoConnect").changed() {
                    self.set_known_autoconnect(value);
                }
            } else {
                ui.label("AutoConnect: unavailable");
            }

            ui.add(
                egui::TextEdit::multiline(&mut self.selected_known_details)
                    .desired_rows(8)
                    .interactive(false),
            );
        }
    }
}

impl eframe::App for IwdGuiApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if !self.initialized {
            self.initialized = true;
            self.refresh_all();
        }

        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("Device");
                egui::ComboBox::from_id_salt("device_selector")
                    .selected_text(self.selected_device_name())
                    .show_ui(ui, |ui| {
                        for device in &self.devices {
                            ui.selectable_value(
                                &mut self.selected_device_path,
                                Some(device.path.clone()),
                                &device.name,
                            );
                        }
                    });

                if ui.button("Refresh Devices").clicked() {
                    self.refresh_all();
                }
            });

            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.active_tab, ActiveTab::Networks, "Networks");
                ui.selectable_value(&mut self.active_tab, ActiveTab::Saved, "Saved");
            });
        });

        egui::TopBottomPanel::bottom("status_panel").show(ctx, |ui| {
            ui.label(self.status_line.as_str());
        });

        egui::CentralPanel::default().show(ctx, |ui| match self.active_tab {
            ActiveTab::Networks => self.draw_networks_tab(ui),
            ActiveTab::Saved => self.draw_saved_tab(ui),
        });
    }
}

#[derive(Debug)]
struct IwdDbus {
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
    fn new() -> Result<Self, String> {
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

    fn list_devices(&self) -> Result<Vec<DeviceInfo>, String> {
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

    fn list_visible_networks(
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

    fn list_known_networks(&self) -> Result<Vec<KnownNetwork>, String> {
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

    fn scan(&self, device_path: &str) -> Result<(), String> {
        let proxy = Proxy::new(&self.conn, IWD_SERVICE, device_path, STATION_IFACE)
            .map_err(|e| e.to_string())?;
        let _: () = proxy.call("Scan", &()).map_err(|e| e.to_string())?;
        Ok(())
    }

    fn connect_network(&self, network_path: &str, passphrase: Option<&str>) -> Result<(), String> {
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

    fn forget_known_network(&self, known_path: &str) -> Result<(), String> {
        let proxy = Proxy::new(&self.conn, IWD_SERVICE, known_path, KNOWN_NETWORK_IFACE)
            .map_err(|e| e.to_string())?;
        let _: () = proxy.call("Forget", &()).map_err(|e| e.to_string())?;
        Ok(())
    }

    fn set_known_autoconnect(&self, known_path: &str, enabled: bool) -> Result<(), String> {
        let proxy = Proxy::new(&self.conn, IWD_SERVICE, known_path, KNOWN_NETWORK_IFACE)
            .map_err(|e| e.to_string())?;
        proxy
            .set_property("AutoConnect", enabled)
            .map_err(|e| e.to_string())
    }
}

fn format_known_network(known: &KnownNetwork) -> String {
    let autoconnect = known
        .autoconnect
        .map(|v| if v { "yes" } else { "no" })
        .unwrap_or("unknown");
    let hidden = known
        .hidden
        .map(|v| if v { "yes" } else { "no" })
        .unwrap_or("unknown");

    format!(
        "Name: {}\nType: {}\nAutoConnect: {}\nHidden: {}\nObject: {}",
        known.name, known.network_type, autoconnect, hidden, known.path
    )
}

fn main() {
    let options = eframe::NativeOptions::default();
    let run = eframe::run_native(
        "iwd-gui",
        options,
        Box::new(|_cc| Ok(Box::new(IwdGuiApp::default()))),
    );

    if let Err(err) = run {
        eprintln!("failed to start GUI: {err}");
    }
}
