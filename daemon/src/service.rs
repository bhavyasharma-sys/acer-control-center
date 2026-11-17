use std::sync::Arc;

use crate::animation::HardwareAnimationBackend;
use crate::backend::HardwareBackend;

pub struct AcerService {
    backend: Arc<dyn HardwareBackend>,
    animation_backend: Arc<dyn HardwareAnimationBackend>,
}

impl AcerService {
    pub fn new(
        backend: Arc<dyn HardwareBackend>,
        animation_backend: Arc<dyn HardwareAnimationBackend>,
    ) -> Self {
        Self {
            backend,
            animation_backend,
        }
    }

    pub fn get_keyboard_brightness(
        &self,
    ) -> Result<u8, String> {
        self.backend.get_keyboard_brightness()
    }

    pub fn set_keyboard_brightness(
        &self,
        value: u8,
    ) -> Result<(), String> {
        self.backend.set_keyboard_brightness(value)
    }

    pub fn set_keyboard_brightness_transient(
        &self,
        value: u8,
    ) -> Result<(), String> {
        self.backend.set_keyboard_brightness_transient(value)
    }

    pub fn get_keyboard_color(
        &self,
    ) -> Result<(u8, u8, u8), String> {
        self.backend.get_keyboard_color()
    }

    pub fn set_keyboard_color(
        &self,
        r: u8,
        g: u8,
        b: u8,
    ) -> Result<(), String> {
        self.backend.set_keyboard_color(r, g, b)
    }

    pub fn set_fan_max(&self) -> Result<(), String> {
        self.backend.set_fan_max()
    }

    pub fn set_fan_auto(&self) -> Result<(), String> {
        self.backend.set_fan_auto()
    }

    pub fn set_power_caps(&self, cpu_watts: u32, gpu_watts: u32) -> (String, String) {
        let cpu = self.backend.set_cpu_power_cap(cpu_watts)
            .map(|()| format!("CPU RAPL setting read back: {cpu_watts} W (actual workload enforcement is not measured)"))
            .unwrap_or_else(|error| format!("CPU cap not applied: {error}"));
        let gpu = self.backend.set_gpu_power_cap(gpu_watts)
            .map(|()| format!("NVIDIA current limit verified: {gpu_watts} W"))
            .unwrap_or_else(|error| format!("GPU cap not applied: {error}"));
        (cpu, gpu)
    }

    pub fn fan_max_selected(&self) -> bool {
        self.backend.fan_max_selected()
    }

    pub fn start_breathing(&self) -> Result<(), String> {
        self.animation_backend.start_breathing()
    }

    pub fn stop_animation(&self) -> Result<(), String> {
        self.animation_backend.stop_animation()
    }

    pub fn breathing_selected(&self) -> bool {
        self.animation_backend.breathing_selected()
    }
}
