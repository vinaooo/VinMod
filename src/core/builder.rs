use std::fmt::{Display, Formatter};
use std::thread;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CpuScheduler {
    CachyOsBore,
    Bore,
    Eevdf,
    Bmq,
    RealTime,
}

impl Default for CpuScheduler {
    fn default() -> Self {
        Self::CachyOsBore
    }
}

impl Display for CpuScheduler {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let value = match self {
            Self::CachyOsBore => "cachyos",
            Self::Bore => "bore",
            Self::Eevdf => "eevdf",
            Self::Bmq => "bmq",
            Self::RealTime => "rt",
        };

        write!(f, "{value}")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LtoMode {
    Thin,
    ThinDist,
    Full,
    None,
}

impl Default for LtoMode {
    fn default() -> Self {
        Self::Thin
    }
}

impl Display for LtoMode {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let value = match self {
            Self::Thin => "thin",
            Self::ThinDist => "thin-dist",
            Self::Full => "full",
            Self::None => "none",
        };

        write!(f, "{value}")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TickType {
    NoHzIdle,
    NoHzFull,
    Periodic,
}

impl Default for TickType {
    fn default() -> Self {
        Self::NoHzIdle
    }
}

impl Display for TickType {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let value = match self {
            Self::NoHzIdle => "nohz_idle",
            Self::NoHzFull => "nohz_full",
            Self::Periodic => "periodic",
        };

        write!(f, "{value}")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PreemptionType {
    Preempt,
    Voluntary,
    PreemptDynamic,
    None,
}

impl Default for PreemptionType {
    fn default() -> Self {
        Self::Preempt
    }
}

impl Display for PreemptionType {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let value = match self {
            Self::Preempt => "preempt",
            Self::Voluntary => "voluntary",
            Self::PreemptDynamic => "preempt_dynamic",
            Self::None => "none",
        };

        write!(f, "{value}")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PackageFormat {
    Debian,
    RedHat,
    Arch,
    Tarball,
}

impl Default for PackageFormat {
    fn default() -> Self {
        Self::Debian
    }
}

impl PackageFormat {
    pub fn from_index(index: u32) -> Self {
        match index {
            1 => Self::RedHat,
            2 => Self::Arch,
            3 => Self::Tarball,
            _ => Self::Debian,
        }
    }
}

impl From<u32> for PackageFormat {
    fn from(value: u32) -> Self {
        Self::from_index(value)
    }
}

impl Display for PackageFormat {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let value = match self {
            Self::Debian => "deb",
            Self::RedHat => "rpm",
            Self::Arch => "pkg.tar.zst",
            Self::Tarball => "tar.gz",
        };

        write!(f, "{value}")
    }
}

#[derive(Debug, Clone)]
pub struct KernelBuildConfig {
    kernel_version: String,
    architecture: String,
    scheduler: CpuScheduler,
    lto: LtoMode,
    hz: u32,
    nr_cpus: u32,
    tick_type: TickType,
    preemption: PreemptionType,
    package_format: PackageFormat,
    system_optimizations: Vec<String>,
}

impl Default for KernelBuildConfig {
    fn default() -> Self {
        Self {
            kernel_version: "6.19.12".to_string(),
            architecture: "native".to_string(),
            scheduler: CpuScheduler::default(),
            lto: LtoMode::default(),
            hz: 1000,
            nr_cpus: thread::available_parallelism().map(|n| n.get()).unwrap_or(16) as u32 * 2,
            tick_type: TickType::default(),
            preemption: PreemptionType::default(),
            package_format: PackageFormat::default(),
            system_optimizations: vec!["O3".to_string(), "tcp_bbr3".to_string()],
        }
    }
}

impl KernelBuildConfig {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_kernel_version(mut self, kernel_version: impl Into<String>) -> Self {
        self.kernel_version = kernel_version.into();
        self
    }

    pub fn with_architecture(mut self, architecture: impl Into<String>) -> Self {
        self.architecture = architecture.into();
        self
    }

    pub fn with_scheduler(mut self, scheduler: CpuScheduler) -> Self {
        self.scheduler = scheduler;
        self
    }

    pub fn with_lto(mut self, lto: LtoMode) -> Self {
        self.lto = lto;
        self
    }

    pub fn with_hz(mut self, hz: u32) -> Self {
        self.hz = hz;
        self
    }

    pub fn with_nr_cpus(mut self, nr_cpus: u32) -> Self {
        self.nr_cpus = nr_cpus.max(1);
        self
    }

    pub fn with_tick_type(mut self, tick_type: TickType) -> Self {
        self.tick_type = tick_type;
        self
    }

    pub fn with_preemption(mut self, preemption: PreemptionType) -> Self {
        self.preemption = preemption;
        self
    }

    pub fn with_package_format(mut self, package_format: PackageFormat) -> Self {
        self.package_format = package_format;
        self
    }

    pub fn with_system_optimizations(mut self, system_optimizations: Vec<String>) -> Self {
        self.system_optimizations = system_optimizations;
        self
    }

    pub fn kernel_version(&self) -> &str {
        &self.kernel_version
    }

    pub fn architecture(&self) -> &str {
        &self.architecture
    }

    pub fn scheduler(&self) -> &CpuScheduler {
        &self.scheduler
    }

    pub fn lto(&self) -> &LtoMode {
        &self.lto
    }

    pub fn hz(&self) -> u32 {
        self.hz
    }

    pub fn nr_cpus(&self) -> u32 {
        self.nr_cpus
    }

    pub fn tick_type(&self) -> &TickType {
        &self.tick_type
    }

    pub fn preemption(&self) -> &PreemptionType {
        &self.preemption
    }

    pub fn package_format(&self) -> &PackageFormat {
        &self.package_format
    }

    pub fn system_optimizations(&self) -> &[String] {
        &self.system_optimizations
    }
}
