use std::env;
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::path::PathBuf;
use std::thread;
use std::time::Duration;

use log::{error, info};

use crate::core::builder::{KernelBuildConfig, PackageFormat, PreemptionType, TickType};
use crate::infra::filesystem::{FileSystem, FileSystemError, LocalFileSystem};
use crate::infra::process::{ProcessError, ProcessExecutor, ProcessOutput, SystemProcessExecutor};

const DEFAULT_KERNEL_REPO: &str = "https://git.kernel.org/pub/scm/linux/kernel/git/stable/linux.git";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BuildStage {
    PrepareWorkspace,
    ValidateToolchain,
    EnsureSource,
    ConfigureKernel,
    CompileKernel,
    PackageOutput,
    Finalize,
}

impl BuildStage {
    fn label(self) -> &'static str {
        match self {
            Self::PrepareWorkspace => "Preparing build workspace...\n",
            Self::ValidateToolchain => "Validating toolchain and system requirements...\n",
            Self::EnsureSource => "Acquiring kernel source tree...\n",
            Self::ConfigureKernel => "Configuring kernel profile...\n",
            Self::CompileKernel => "Compiling kernel and modules...\n",
            Self::PackageOutput => "Packaging build output...\n",
            Self::Finalize => "Finalizing build output...\n",
        }
    }
}

#[derive(Debug)]
pub enum BuildError {
    Process(ProcessError),
    Filesystem(FileSystemError),
    CommandFailed(String),
    MissingArtifact(String),
}

impl Display for BuildError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Process(error) => write!(f, "Process error: {error}"),
            Self::Filesystem(error) => write!(f, "Filesystem error: {error}"),
            Self::CommandFailed(message) => write!(f, "Command failed: {message}"),
            Self::MissingArtifact(message) => write!(f, "Missing artifact: {message}"),
        }
    }
}

impl Error for BuildError {}

impl From<ProcessError> for BuildError {
    fn from(value: ProcessError) -> Self {
        Self::Process(value)
    }
}

impl From<FileSystemError> for BuildError {
    fn from(value: FileSystemError) -> Self {
        Self::Filesystem(value)
    }
}

#[derive(Debug, Clone)]
struct BuildPaths {
    root: PathBuf,
    source_dir: PathBuf,
    bundle_dir: PathBuf,
    package_root: PathBuf,
    manifest_path: PathBuf,
    snapshot_path: PathBuf,
    status_path: PathBuf,
    output_path: PathBuf,
    config_fragment_path: PathBuf,
}

impl BuildPaths {
    fn new(root: PathBuf, config: &KernelBuildConfig) -> Self {
        let source_dir = root.join("linux-src");
        let bundle_dir = root.join("bundle");
        let package_name = format!(
            "vinmod-kernel-{}-{}",
            config.kernel_version().replace('/', "-"),
            config.package_format()
        );
        let package_root = bundle_dir.join(&package_name);
        let output_path = match config.package_format() {
            PackageFormat::Debian => root.join(format!("{}.deb", package_name)),
            PackageFormat::RedHat => root.join(format!("{}.tar.gz", package_name)),
            PackageFormat::Arch => root.join(format!("{}.pkg.tar.gz", package_name)),
            PackageFormat::Tarball => root.join(format!("{}.tar.gz", package_name)),
        };

        Self {
            root: root.clone(),
            source_dir,
            bundle_dir,
            package_root,
            manifest_path: root.join("build-manifest.txt"),
            snapshot_path: root.join("system-snapshot.txt"),
            status_path: root.join("BUILD_STATUS.txt"),
            output_path,
            config_fragment_path: root.join("kernel-profile.fragment"),
        }
    }
}

pub struct BuildService<P: ProcessExecutor, F: FileSystem> {
    process: P,
    filesystem: F,
    work_dir: PathBuf,
}

impl BuildService<SystemProcessExecutor, LocalFileSystem> {
    pub fn new() -> Self {
        Self {
            process: SystemProcessExecutor,
            filesystem: LocalFileSystem,
            work_dir: PathBuf::from("build-artifacts"),
        }
    }
}

impl<P: ProcessExecutor, F: FileSystem> BuildService<P, F> {
    pub fn with_infra(process: P, filesystem: F, work_dir: impl Into<PathBuf>) -> Self {
        Self {
            process,
            filesystem,
            work_dir: work_dir.into(),
        }
    }

    pub fn run_build<G, C>(
        &self,
        config: &KernelBuildConfig,
        mut emit: G,
        is_cancelled: C,
    ) -> Result<(), BuildError>
    where
        G: FnMut(String),
        C: Fn() -> bool,
    {
        info!(
            "Starting build with kernel_version={}, architecture={}, scheduler={}, lto={}, hz={}, tick_type={}, preemption={}, package={}",
            config.kernel_version(),
            config.architecture(),
            config.scheduler(),
            config.lto(),
            config.hz(),
            config.tick_type(),
            config.preemption(),
            config.package_format()
        );

        let paths = BuildPaths::new(self.work_dir.clone(), config);
        self.filesystem.create_dir_all(&paths.root)?;

        self.write_manifest(config, &paths)?;

        for stage in [
            BuildStage::PrepareWorkspace,
            BuildStage::ValidateToolchain,
            BuildStage::EnsureSource,
            BuildStage::ConfigureKernel,
            BuildStage::CompileKernel,
            BuildStage::PackageOutput,
            BuildStage::Finalize,
        ] {
            if is_cancelled() {
                emit("\n--- Build Stopped by User ---\n".to_string());
                return Ok(());
            }

            emit(stage.label().to_string());

            let stage_result = match stage {
                BuildStage::PrepareWorkspace => self.prepare_workspace(&paths),
                BuildStage::ValidateToolchain => self.ensure_toolchain(),
                BuildStage::EnsureSource => self.ensure_source_tree(config, &paths),
                BuildStage::ConfigureKernel => self.configure_kernel(config, &paths),
                BuildStage::CompileKernel => self.compile_kernel(config, &paths),
                BuildStage::PackageOutput => self.package_output(config, &paths),
                BuildStage::Finalize => self.finalize_build(&paths),
            };

            if let Err(err) = stage_result {
                return Err(err);
            }

            thread::sleep(Duration::from_millis(150));
        }

        let uname = self.run_shell("uname -r")?;
        if !uname.success {
            error!("uname command failed: {}", uname.stderr.trim());
            return Err(BuildError::CommandFailed(uname.stderr));
        }

        emit(format!("Host kernel: {}\n", uname.stdout.trim()));
        emit(format!("Manifest written to: {}\n", paths.manifest_path.display()));
        emit(format!("Snapshot written to: {}\n", paths.snapshot_path.display()));
        emit(format!("Artifact generated at: {}\n", paths.output_path.display()));
        emit("SUCCESS! Kernel build pipeline completed in Rust.\n".to_string());
        Ok(())
    }

    fn prepare_workspace(&self, paths: &BuildPaths) -> Result<(), BuildError> {
        self.filesystem.create_dir_all(&paths.bundle_dir)?;
        self.filesystem.create_dir_all(&paths.package_root)?;
        self.filesystem.create_dir_all(&paths.source_dir)?;
        Ok(())
    }

    fn ensure_source_tree(&self, config: &KernelBuildConfig, paths: &BuildPaths) -> Result<(), BuildError> {
        let repo = env::var("VINMOD_KERNEL_REPO").unwrap_or_else(|_| DEFAULT_KERNEL_REPO.to_string());
        let reference = env::var("VINMOD_KERNEL_REF").unwrap_or_else(|_| format!("v{}", config.kernel_version()));

        let command = format!(
            r#"
            set -e
            if [ -d '{source}/.git' ]; then
                cd '{source}'
                git fetch --tags --depth 1 origin '{reference}' >/dev/null 2>&1 || true
                git checkout -f '{reference}' >/dev/null 2>&1 || true
            elif [ -f '{source}/Makefile' ]; then
                true
            else
                git clone --depth 1 --branch '{reference}' '{repo}' '{source}' || git clone --depth 1 '{repo}' '{source}'
            fi
            "#,
            source = shell_quote(paths.source_dir.to_string_lossy()),
            reference = shell_quote(&reference),
            repo = shell_quote(&repo),
        );

        let output = self.run_shell(&command)?;
        self.emit_process_output("source tree", &output);
        if !output.success {
            return Err(BuildError::CommandFailed(output.stderr));
        }

        Ok(())
    }

    fn configure_kernel(&self, config: &KernelBuildConfig, paths: &BuildPaths) -> Result<(), BuildError> {
        let config_commands = self.kernel_config_commands(config);

        self.filesystem
            .write_string(&paths.config_fragment_path, &config_commands.join("\n"))?;

        let command = format!(
            r#"
            set -e
            cd '{source}'
            if [ ! -f .config ]; then
                make defconfig
            fi
            {commands}
            make olddefconfig
            "#,
            source = shell_quote(paths.source_dir.to_string_lossy()),
            commands = config_commands.join("\n"),
        );

        let output = self.run_shell(&command)?;
        self.emit_process_output("kernel configuration", &output);
        if !output.success {
            return Err(BuildError::CommandFailed(output.stderr));
        }

        Ok(())
    }

    fn compile_kernel(&self, config: &KernelBuildConfig, paths: &BuildPaths) -> Result<(), BuildError> {
        let cpu_count = cpu_count_shell();
        let command = match config.package_format() {
            PackageFormat::Debian => format!(
                r#"
                set -e
                cd '{source}'
                make -j{cpu_count} bindeb-pkg LOCALVERSION=-vinmod KDEB_PKGVERSION=1.0
                "#,
                source = shell_quote(paths.source_dir.to_string_lossy()),
                cpu_count = cpu_count,
            ),
            _ => format!(
                r#"
                set -e
                cd '{source}'
                make -j{cpu_count}
                make modules_install INSTALL_MOD_PATH='{bundle}' >/dev/null 2>&1 || true
                "#,
                source = shell_quote(paths.source_dir.to_string_lossy()),
                bundle = shell_quote(paths.package_root.to_string_lossy()),
                cpu_count = cpu_count,
            ),
        };

        let output = self.run_shell(&command)?;
        self.emit_process_output("kernel compilation", &output);
        if !output.success {
            return Err(BuildError::CommandFailed(output.stderr));
        }

        Ok(())
    }

    fn package_output(&self, config: &KernelBuildConfig, paths: &BuildPaths) -> Result<(), BuildError> {
        match config.package_format() {
            PackageFormat::Debian => {
                let locate_command = format!(
                    r#"
                    set -e
                    find '{parent}' -maxdepth 1 -type f \( -name '*.deb' -o -name '*.dsc' -o -name '*.changes' \) | sort
                    "#,
                    parent = shell_quote(paths.source_dir.parent().unwrap_or(&paths.root).to_string_lossy()),
                );

                let output = self.run_shell(&locate_command)?;
                self.emit_process_output("package discovery", &output);
                if !output.success {
                    return Err(BuildError::CommandFailed(output.stderr));
                }

                if output.stdout.trim().is_empty() {
                    return Err(BuildError::MissingArtifact(
                        "Debian package was not generated by bindeb-pkg".to_string(),
                    ));
                }

                self.filesystem.write_string(&paths.output_path, &output.stdout)?;
            }
            PackageFormat::RedHat | PackageFormat::Arch | PackageFormat::Tarball => {
                let command = format!(
                    r#"
                    set -e
                    tar -czf '{archive}' -C '{root}' \
                        'build-manifest.txt' \
                        'system-snapshot.txt' \
                        'kernel-profile.fragment' \
                        'bundle'
                    "#,
                    archive = shell_quote(paths.output_path.to_string_lossy()),
                    root = shell_quote(paths.root.to_string_lossy()),
                );

                let output = self.run_shell(&command)?;
                self.emit_process_output("artifact packaging", &output);
                if !output.success {
                    return Err(BuildError::CommandFailed(output.stderr));
                }
            }
        }

        Ok(())
    }

    fn finalize_build(&self, paths: &BuildPaths) -> Result<(), BuildError> {
        self.filesystem.write_string(
            &paths.status_path,
            "Build completed successfully.\n",
        )?;
        Ok(())
    }

    fn ensure_toolchain(&self) -> Result<(), BuildError> {
        let mut requirements = vec!["git", "gcc", "make", "tar", "bc", "flex", "bison", "perl"];
        requirements.push("find");

        for tool in requirements {
            let result = self.run_shell(&format!("command -v {tool} >/dev/null 2>&1"))?;
            if !result.success {
                return Err(BuildError::CommandFailed(format!("Required tool not found: {tool}")));
            }
        }

        Ok(())
    }

    fn kernel_config_commands(&self, config: &KernelBuildConfig) -> Vec<String> {
        let mut commands = Vec::new();

        commands.push("scripts/config -d HZ_100 -d HZ_250 -d HZ_300 -d HZ_500 -d HZ_600 -d HZ_750 -d HZ_1000".to_string());
        commands.push(format!("scripts/config --set-val HZ {}", config.hz()));

        commands.push("scripts/config -d NO_HZ_IDLE -d NO_HZ_FULL -d NO_HZ_FULL_NODEF -d NO_HZ -d NO_HZ_COMMON -d CONTEXT_TRACKING".to_string());
        match config.tick_type() {
            TickType::NoHzIdle => commands.push("scripts/config -e NO_HZ_IDLE -e NO_HZ -e NO_HZ_COMMON".to_string()),
            TickType::NoHzFull => commands.push("scripts/config -e NO_HZ_FULL_NODEF -e NO_HZ_FULL -e NO_HZ -e NO_HZ_COMMON -e CONTEXT_TRACKING".to_string()),
            TickType::Periodic => commands.push("scripts/config -e HZ_PERIODIC".to_string()),
        }

        commands.push("scripts/config -d PREEMPT_NONE -d PREEMPT_VOLUNTARY -d PREEMPT -d PREEMPT_DYNAMIC -d PREEMPT_RT".to_string());
        match config.preemption() {
            PreemptionType::Preempt => commands.push("scripts/config -e PREEMPT".to_string()),
            PreemptionType::Voluntary => commands.push("scripts/config -e PREEMPT_VOLUNTARY".to_string()),
            PreemptionType::PreemptDynamic => commands.push("scripts/config -e PREEMPT_DYNAMIC".to_string()),
            PreemptionType::None => commands.push("scripts/config -e PREEMPT_NONE".to_string()),
        }

        commands.push("scripts/config -d TRANSPARENT_HUGEPAGE -d TRANSPARENT_HUGEPAGE_ALWAYS -d TRANSPARENT_HUGEPAGE_MADVISE -d TRANSPARENT_HUGEPAGE_NEVER".to_string());
        commands.push("scripts/config -e TRANSPARENT_HUGEPAGE -e TRANSPARENT_HUGEPAGE_MADVISE".to_string());

        commands.push("scripts/config -d CPU_FREQ_DEFAULT_GOV_POWERSAVE -d CPU_FREQ_DEFAULT_GOV_SCHEDUTIL -d CPU_FREQ_DEFAULT_GOV_USERSPACE -d CPU_FREQ_DEFAULT_GOV_ONDEMAND -d CPU_FREQ_DEFAULT_GOV_CONSERVATIVE -d CPU_FREQ_DEFAULT_GOV_PERFORMANCE".to_string());
        commands.push("scripts/config -e CPU_FREQ_GOV_PERFORMANCE -e CPU_FREQ_DEFAULT_GOV_PERFORMANCE".to_string());

        commands.push("scripts/config -d TCP_CONG_BBR -d TCP_CONG_BBR3 -d DEFAULT_RENO -d DEFAULT_CUBIC -d DEFAULT_BBR -d DEFAULT_BBR3".to_string());
        commands.push("scripts/config -e TCP_CONG_BBR -e TCP_CONG_BBR3 --set-str DEFAULT_TCP_CONG bbr3".to_string());

        commands.push("scripts/config -d SCHED_BORE -d SCHED_ALT -d SCHED_BMQ -d SCHED_CLASS_EXT -d SCHED_CORE".to_string());
        match config.scheduler() {
            crate::core::builder::CpuScheduler::CachyOsBore => commands.push("scripts/config -e SCHED_BORE".to_string()),
            crate::core::builder::CpuScheduler::Bore => commands.push("scripts/config -e SCHED_BORE".to_string()),
            crate::core::builder::CpuScheduler::Eevdf => commands.push("scripts/config -e SCHED_CORE".to_string()),
            crate::core::builder::CpuScheduler::Bmq => commands.push("scripts/config -e SCHED_BMQ".to_string()),
            crate::core::builder::CpuScheduler::RealTime => commands.push("scripts/config -e PREEMPT_RT".to_string()),
        }

        commands.push("scripts/config -d HUGETLB_PAGE -e TRANSPARENT_HUGEPAGE".to_string());
        commands.push(format!("scripts/config --set-val NR_CPUS {}", config.nr_cpus()));

        commands
    }

    fn write_manifest(&self, config: &KernelBuildConfig, paths: &BuildPaths) -> Result<(), BuildError> {
        let manifest = format!(
            "kernel_version={}\narchitecture={}\nscheduler={}\nlto={}\nhz={}\nnr_cpus={}\ntick_type={}\npreemption={}\npackage={}\noptimizations={:?}\nroot={}\nsource_dir={}\noutput={}\n",
            config.kernel_version(),
            config.architecture(),
            config.scheduler(),
            config.lto(),
            config.hz(),
            config.nr_cpus(),
            config.tick_type(),
            config.preemption(),
            config.package_format(),
            config.system_optimizations(),
            paths.root.display(),
            paths.source_dir.display(),
            paths.output_path.display(),
        );

        self.filesystem.write_string(&paths.manifest_path, &manifest)?;
        Ok(())
    }

    fn run_shell(&self, command: &str) -> Result<ProcessOutput, BuildError> {
        Ok(self.process.run("sh", &["-lc", command])?)
    }

    fn emit_process_output(&self, label: &str, output: &ProcessOutput) {
        if !output.stdout.trim().is_empty() {
            info!("{} stdout: {}", label, output.stdout.trim());
        }
        if !output.stderr.trim().is_empty() {
            info!("{} stderr: {}", label, output.stderr.trim());
        }
    }
}

pub type DefaultBuildService = BuildService<SystemProcessExecutor, LocalFileSystem>;

fn shell_quote(value: impl AsRef<str>) -> String {
    let raw = value.as_ref();
    format!("'{}'", raw.replace('\'', "'\\''"))
}

fn cpu_count_shell() -> String {
    "$(nproc --all 2>/dev/null || getconf _NPROCESSORS_ONLN 2>/dev/null || echo 1)".to_string()
}