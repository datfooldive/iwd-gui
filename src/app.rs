use eframe::egui;

use crate::dbus::IwdDbus;
use crate::models::{ActiveTab, DeviceInfo, KnownNetwork, VisibleNetwork};

#[derive(Debug)]
pub(crate) struct IwdGuiApp {
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
