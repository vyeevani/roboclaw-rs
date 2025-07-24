use eframe::egui;
use std::io;
use std::time::{Duration, Instant};
use roboclaw::{Roboclaw, StatusFlags, ConfigFlags, BufferStatus};
use serialport::{SerialPort, SerialPortType};

pub struct RoboclawGUI {
    // Connection settings
    port_name: String,
    baud_rate: u32,
    connected: bool,
    
    // Motor controls
    m1_speed: f32,
    m2_speed: f32,
    mixed_speed: f32,
    mixed_turn: f32,
    
    // Status displays
    main_battery_voltage: Option<f32>,
    logic_battery_voltage: Option<f32>,
    encoder_m1: Option<u32>,
    encoder_m2: Option<u32>,
    status_flags: Option<StatusFlags>,
    config_flags: Option<ConfigFlags>,
    buffer_status: Option<(BufferStatus, BufferStatus)>,
    
    // Control state
    last_update: Instant,
    status_message: String,
    
    // Connection state 
    roboclaw: Option<Roboclaw>,
}

impl Default for RoboclawGUI {
    fn default() -> Self {
        Self {
            port_name: "/dev/tty.usbmodem101".to_owned(),
            baud_rate: 38400,
            connected: false,
            m1_speed: 0.0,
            m2_speed: 0.0,
            mixed_speed: 0.0,
            mixed_turn: 0.0,
            main_battery_voltage: None,
            logic_battery_voltage: None,
            encoder_m1: None,
            encoder_m2: None,
            status_flags: None,
            config_flags: None,
            buffer_status: None,
            last_update: Instant::now(),
            status_message: "Disconnected".to_owned(),
            roboclaw: None,
        }
    }
}

impl RoboclawGUI {
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        Default::default()
    }

    fn connect(&mut self) {
        let port = serialport::new(&self.port_name, self.baud_rate)
            .timeout(Duration::from_millis(10))
            .open()
            .map_err(|e| {
                self.status_message = format!("Failed to open port: {}", e);
            })
            .ok();

        let roboclaw = port
            .map(|port| Roboclaw::new(port));

        if let Some(roboclaw) = roboclaw {
            self.roboclaw = Some(roboclaw);
            self.connected = true;
            self.status_message = "Connected - Testing communication...".to_owned();
            // Optionally, test communication here using map or and_then if needed
            // self.roboclaw.as_mut().and_then(|roboclaw| {
            //     roboclaw.read_main_battery_voltage().map(|voltage| {
            //         self.status_message = format!("✓ Connected and communicating - Battery: {:.1}V", voltage);
            //     }).map_err(|e| {
            //         self.status_message = format!("⚠ Connected but communication error: {}", e);
            //     }).ok()
            // });
        } else if self.status_message.is_empty() {
            self.status_message = "Failed to initialize Roboclaw".to_owned();
        }
    }

    fn disconnect(&mut self) {
        self.roboclaw = None;
        self.connected = false;
        self.status_message = "Disconnected".to_owned();
    }

    fn emergency_stop(&mut self) {
        if let Some(ref mut roboclaw) = self.roboclaw {
            // Stop both motors
            let _ = roboclaw.speed_m1_m2(0, 0);
            self.m1_speed = 0.0;
            self.m2_speed = 0.0;
            self.mixed_speed = 0.0;
            self.mixed_turn = 0.0;
        }
    }

    fn update_motor_speeds(&mut self) {
        if let Some(ref mut roboclaw) = self.roboclaw {
            let m1_speed_i32 = (self.m1_speed * 1000.0) as i32;
            let m2_speed_i32 = (self.m2_speed * 1000.0) as i32;
            
            match roboclaw.speed_m1_m2(m1_speed_i32, m2_speed_i32) {
                Ok(()) => {
                    // Success - don't change status message if it's showing other important info
                    if !self.status_message.contains("Failed to read") {
                        self.status_message = "Connected".to_owned();
                    }
                },
                Err(e) => {
                    self.status_message = format!("Motor control error: {} (M1:{}, M2:{})", e, m1_speed_i32, m2_speed_i32);
                }
            }
        }
    }

    fn update_mixed_control(&mut self) {
        if let Some(ref mut roboclaw) = self.roboclaw {
            // Convert mixed controls to individual motor speeds
            let base_speed = self.mixed_speed * 1000.0;
            let turn_adjustment = self.mixed_turn * 500.0; // Reduced turning sensitivity
            
            let left_speed = (base_speed - turn_adjustment) as i32;
            let right_speed = (base_speed + turn_adjustment) as i32;
            
            match roboclaw.speed_m1_m2(left_speed, right_speed) {
                Ok(()) => {
                    // Success - don't change status message if it's showing other important info
                    if !self.status_message.contains("Failed to read") {
                        self.status_message = "Connected".to_owned();
                    }
                },
                Err(e) => {
                    self.status_message = format!("Mixed control error: {} (L:{}, R:{})", e, left_speed, right_speed);
                }
            }
        }
    }

    fn read_status(&mut self) {
        if let Some(ref mut roboclaw) = self.roboclaw {
            // Reduce polling frequency if we're having communication errors
            let polling_interval = if self.status_message.contains("crc error") || 
                                     self.status_message.contains("Failed to read") {
                Duration::from_millis(2000) // Slower polling when errors occur
            } else {
                Duration::from_millis(500)  // Normal polling
            };
            
            // Update readings periodically
            if self.last_update.elapsed() > polling_interval {
                // Read battery voltages
                if let Ok(voltage) = roboclaw.read_main_battery_voltage() {
                    self.main_battery_voltage = Some(voltage);
                } else {
                    // Don't overwrite motor control errors with battery read errors
                    if !self.status_message.contains("Motor control error") && !self.status_message.contains("Mixed control error") {
                        self.status_message = "Failed to read main battery voltage".to_owned();
                    }
                }
                
                if let Ok(voltage) = roboclaw.read_logic_battery_voltage() {
                    self.logic_battery_voltage = Some(voltage);
                } else {
                    if !self.status_message.contains("Motor control error") && !self.status_message.contains("Mixed control error") {
                        self.status_message = "Failed to read logic battery voltage".to_owned();
                    }
                }
                
                // Read encoders
                if let Ok((enc1, enc2)) = roboclaw.read_encoders() {
                    self.encoder_m1 = Some(enc1);
                    self.encoder_m2 = Some(enc2);
                } else {
                    if !self.status_message.contains("Motor control error") && !self.status_message.contains("Mixed control error") {
                        self.status_message = "Failed to read encoders".to_owned();
                    }
                }
                
                // Read status flags - this is the important one for motor errors
                match roboclaw.read_error() {
                    Ok(flags) => {
                        self.status_flags = Some(flags);
                        // If we successfully read status and there are no motor errors in status_message, show "Connected"
                        if !self.status_message.contains("Motor control error") && !self.status_message.contains("Mixed control error") {
                            self.status_message = "Connected".to_owned();
                        }
                    },
                    Err(e) => {
                        // This is likely where the "crc error" is coming from
                        if !self.status_message.contains("Motor control error") && !self.status_message.contains("Mixed control error") {
                            self.status_message = format!("Failed to read error status: {}", e);
                        }
                        // Keep the last known status flags rather than clearing them
                    }
                }
                
                // Read config
                if let Ok(config) = roboclaw.get_config() {
                    self.config_flags = Some(config);
                } else {
                    if !self.status_message.contains("Motor control error") && !self.status_message.contains("Mixed control error") && !self.status_message.contains("Failed to read error status") {
                        self.status_message = "Failed to read config".to_owned();
                    }
                }
                
                // Read buffer status
                if let Ok(buffers) = roboclaw.read_buffers() {
                    self.buffer_status = Some(buffers);
                } else {
                    if !self.status_message.contains("Motor control error") && !self.status_message.contains("Mixed control error") && !self.status_message.contains("Failed to read error status") {
                        self.status_message = "Failed to read buffers".to_owned();
                    }
                }
                
                self.last_update = Instant::now();
            }
        }
    }
}

// Helper function to display config flags in a user-friendly way
fn show_config_flags(ui: &mut egui::Ui, config_flags: &ConfigFlags) {
    use roboclaw::ConfigFlags;

    ui.heading("Config Flags");
    ui.label(format!("Raw: 0x{:04X}", config_flags.bits()));

    // Show each flag with a checkbox or label
    let flags = [
        (ConfigFlags::RC_MODE, "RC Mode"),
        (ConfigFlags::ANALOG_MODE, "Analog Mode"),
        (ConfigFlags::SIMPLE_SERIAL_MODE, "Simple Serial Mode"),
        (ConfigFlags::PACKET_SERIAL_MODE, "Packet Serial Mode"),
        (ConfigFlags::BATTERY_MODE_OFF, "Battery Mode Off"),
        (ConfigFlags::BATTERY_MODE_AUTO, "Battery Mode Auto"),
        (ConfigFlags::BATTERY_MODE_2_CELL, "Battery Mode 2 Cell"),
        (ConfigFlags::BATTERY_MODE_3_CELL, "Battery Mode 3 Cell"),
        (ConfigFlags::BATTERY_MODE_4_CELL, "Battery Mode 4 Cell"),
        (ConfigFlags::BATTERY_MODE_5_CELL, "Battery Mode 5 Cell"),
        (ConfigFlags::BATTERY_MODE_6_CELL, "Battery Mode 6 Cell"),
        (ConfigFlags::BATTERY_MODE_7_CELL, "Battery Mode 7 Cell"),
        (ConfigFlags::MIXING, "Mixing"),
        (ConfigFlags::EXPONENTIAL, "Exponential"),
        (ConfigFlags::MCU, "MCU"),
        (ConfigFlags::BAUDRATE_2400, "Baudrate 2400"),
        (ConfigFlags::BAUDRATE_9600, "Baudrate 9600"),
        (ConfigFlags::BAUDRATE_19200, "Baudrate 19200"),
        (ConfigFlags::BAUDRATE_38400, "Baudrate 38400"),
        (ConfigFlags::BAUDRATE_57600, "Baudrate 57600"),
        (ConfigFlags::BAUDRATE_115200, "Baudrate 115200"),
        (ConfigFlags::BAUDRATE_230400, "Baudrate 230400"),
        (ConfigFlags::BAUDRATE_460800, "Baudrate 460800"),
        (ConfigFlags::FLIPSWITCH, "Flip Switch"),
        (ConfigFlags::SLAVE_MODE, "Slave Mode"),
        (ConfigFlags::RELAY_MODE, "Relay Mode"),
        (ConfigFlags::SWAP_ENCODERS, "Swap Encoders"),
        (ConfigFlags::SWAP_BUTTONS, "Swap Buttons"),
        (ConfigFlags::MULTI_UNIT_MODE, "Multi Unit Mode"),
    ];

    for (flag, label) in flags.iter() {
        let enabled = config_flags.contains(*flag);
        ui.horizontal(|ui| {
            ui.checkbox(&mut enabled.clone(), *label);
        });
    }
}

impl eframe::App for RoboclawGUI {

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Update status readings
        self.read_status();

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Roboclaw Motor Controller");
            
            ui.separator();
            
            // Connection panel
            ui.horizontal(|ui| {
                ui.label("Port:");
                ui.text_edit_singleline(&mut self.port_name);
                ui.label("Baud:");
                ui.add(egui::DragValue::new(&mut self.baud_rate).speed(100));
                
                if self.connected {
                    if ui.button("Disconnect").clicked() {
                        self.disconnect();
                    }
                } else {
                    if ui.button("Connect").clicked() {
                        self.connect();
                    }
                }
            });
            
            // Baud rate presets for CRC troubleshooting
            if !self.connected {
                ui.horizontal(|ui| {
                    ui.label("Common baud rates:");
                    if ui.small_button("2400").clicked() { self.baud_rate = 2400; }
                    if ui.small_button("9600").clicked() { self.baud_rate = 9600; }
                    if ui.small_button("19200").clicked() { self.baud_rate = 19200; }
                    if ui.small_button("38400").clicked() { self.baud_rate = 38400; }
                    if ui.small_button("57600").clicked() { self.baud_rate = 57600; }
                    if ui.small_button("115200").clicked() { self.baud_rate = 115200; }
                    if ui.small_button("230400").clicked() { self.baud_rate = 230400; }
                    if ui.small_button("460800").clicked() { self.baud_rate = 460800; }
                });
            }
            
            ui.label(format!("Status: {}", self.status_message));
            
            if !self.connected {
                ui.label("Connect to a Roboclaw device to control motors");
                return;
            }
            
            ui.separator();
            
            // Emergency stop
            ui.horizontal(|ui| {
                if ui.add_sized([100.0, 40.0], egui::Button::new("🛑 EMERGENCY STOP")).clicked() {
                    self.emergency_stop();
                }
                ui.label("Stops all motors immediately");
            });
            
            ui.separator();
            
            // Motor control tabs
            ui.horizontal(|ui| {
                ui.group(|ui| {
                    ui.vertical(|ui| {
                        ui.heading("Individual Motor Control");
                        
                        ui.horizontal(|ui| {
                            ui.label("M1 Speed:");
                            if ui.add(egui::Slider::new(&mut self.m1_speed, -100.0..=100.0).suffix("%")).changed() {
                                self.update_motor_speeds();
                            }
                        });
                        
                        ui.horizontal(|ui| {
                            ui.label("M2 Speed:");
                            if ui.add(egui::Slider::new(&mut self.m2_speed, -100.0..=100.0).suffix("%")).changed() {
                                self.update_motor_speeds();
                            }
                        });
                        
                        if ui.button("Stop Both Motors").clicked() {
                            self.m1_speed = 0.0;
                            self.m2_speed = 0.0;
                            self.update_motor_speeds();
                        }
                    });
                });
                
                ui.group(|ui| {
                    ui.vertical(|ui| {
                        ui.heading("Mixed Motor Control");
                        
                        ui.horizontal(|ui| {
                            ui.label("Speed:");
                            if ui.add(egui::Slider::new(&mut self.mixed_speed, -100.0..=100.0).suffix("%")).changed() {
                                self.update_mixed_control();
                            }
                        });
                        
                        ui.horizontal(|ui| {
                            ui.label("Turn:");
                            if ui.add(egui::Slider::new(&mut self.mixed_turn, -100.0..=100.0).suffix("%")).changed() {
                                self.update_mixed_control();
                            }
                        });
                        
                        if ui.button("Stop Mixed").clicked() {
                            self.mixed_speed = 0.0;
                            self.mixed_turn = 0.0;
                            self.update_mixed_control();
                        }
                    });
                });
            });
            
            ui.separator();
            
            // Status information
            ui.horizontal(|ui| {
                ui.group(|ui| {
                    ui.vertical(|ui| {
                        ui.heading("Battery Status");
                        if let Some(voltage) = self.main_battery_voltage {
                            ui.label(format!("Main Battery: {:.1}V", voltage));
                        } else {
                            ui.label("Main Battery: ---");
                        }
                        
                        if let Some(voltage) = self.logic_battery_voltage {
                            ui.label(format!("Logic Battery: {:.1}V", voltage));
                        } else {
                            ui.label("Logic Battery: ---");
                        }
                    });
                });
                
                ui.group(|ui| {
                    ui.vertical(|ui| {
                        ui.heading("Encoders");
                        if let Some(enc) = self.encoder_m1 {
                            ui.label(format!("M1 Encoder: {}", enc));
                        } else {
                            ui.label("M1 Encoder: ---");
                        }
                        
                        if let Some(enc) = self.encoder_m2 {
                            ui.label(format!("M2 Encoder: {}", enc));
                        } else {
                            ui.label("M2 Encoder: ---");
                        }
                        
                        if ui.button("Reset Encoders").clicked() {
                            if let Some(ref mut roboclaw) = self.roboclaw {
                                if let Err(e) = roboclaw.reset_encoders() {
                                    self.status_message = format!("Reset encoders error: {}", e);
                                }
                            }
                        }
                    });
                });
                
                ui.group(|ui| {
                    ui.vertical(|ui| {
                        ui.heading("System Status");
                        ui.label(&self.status_message);
                        
                        if let Some(status) = &self.status_flags {
                            ui.label(format!("Raw Status: 0x{:04X}", status.bits()));
                        }
                        
                        if let Some((buf1, buf2)) = &self.buffer_status {
                            ui.label(format!("Buffer 1: {:?}", buf1));
                            ui.label(format!("Buffer 2: {:?}", buf2));
                        }
                    });
                });

                // // Config flags section
                // ui.group(|ui| {
                //     ui.vertical(|ui| {
                //         ui.heading("Config");
                //         if let Some(config_flags) = &self.config_flags {
                //             show_config_flags(ui, config_flags);
                //         } else {
                //             ui.label("Config: ---");
                //         }
                //     });
                // });
            });
        });
        // Request repaint for real-time updates
        ctx.request_repaint_after(Duration::from_millis(100));
    }
}

fn main() -> Result<(), eframe::Error> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1200.0, 800.0]),
        ..Default::default()
    };
    
    eframe::run_native(
        "Roboclaw Motor Controller",
        options,
        Box::new(|cc| Box::new(RoboclawGUI::new(cc))),
    )
} 