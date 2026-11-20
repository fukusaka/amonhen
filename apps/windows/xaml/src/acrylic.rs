//! 窓の後ろを透かす Acrylic（`DesktopAcrylicController`）。Window.SystemBackdrop の Acrylic は色の層（Tint・
//! Luminosity）の濃さを変えられず、アプリが塗る地を薄くしても透けにくい。ここでは色の層を 0 にし、透け具合は
//! アプリが塗る地の濃さだけで決める（ぼかしは Acrylic のまま）。

use crate::bindings::{
    DesktopAcrylicController, ICompositionSupportsSystemBackdrop, SystemBackdropConfiguration,
    SystemBackdropTheme,
};
use windows_core::{IInspectable, Interface, Result};

/// 当てている Acrylic。落とすと外す。
pub struct Acrylic {
    controller: DesktopAcrylicController,
    configuration: SystemBackdropConfiguration,
}

impl Acrylic {
    /// `window`（WinUI の Window）の後ろに、色の層のない Acrylic を敷く。
    pub fn new(window: &IInspectable, dark: bool) -> Result<Self> {
        let controller = DesktopAcrylicController::new()?;
        controller.SetTintOpacity(0.0)?;
        controller.SetLuminosityOpacity(0.0)?;
        let configuration = SystemBackdropConfiguration::new()?;
        // 窓が前にないときも同じに見せる（既定では、前にない窓の Acrylic は単色になる）。
        configuration.SetIsInputActive(true)?;
        let acrylic = Self {
            controller,
            configuration,
        };
        acrylic.set_dark(dark)?;
        acrylic
            .controller
            .SetSystemBackdropConfiguration(&acrylic.configuration)?;
        acrylic
            .controller
            .AddSystemBackdropTarget(&window.cast::<ICompositionSupportsSystemBackdrop>()?)?;
        Ok(acrylic)
    }

    /// 外観を合わせる。
    pub fn set_dark(&self, dark: bool) -> Result<()> {
        self.configuration.SetTheme(if dark {
            SystemBackdropTheme::Dark
        } else {
            SystemBackdropTheme::Light
        })
    }
}

impl Drop for Acrylic {
    fn drop(&mut self) {
        let _ = self.controller.RemoveAllSystemBackdropTargets();
    }
}
