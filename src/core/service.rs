use std::error::Error;
use std::fmt::{Display, Formatter};
use std::path::PathBuf;
use std::thread;
use std::time::Duration;

use log::{error, info};

use crate::core::builder::{KernelBuildConfig, PackageFormat, PreemptionType, TickType};
use crate::infra::filesystem::{FileSystem, FileSystemError, LocalFileSystem};
use crate::infra::process::{ProcessError, ProcessExecutor, ProcessOutput, SystemProcessExecutor};

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
            Self::PackageOutput => "Packaging Debian kernel + headers...\n",
            Self::Finalize => "Finalizing build output...\n",
        }
    }

    fn key(self) -> &'static str {
        match self {
            Self::PrepareWorkspace => "prepare-workspace",
            Self::ValidateToolchain => "validate-toolchain",
            Self::EnsureSource => "ensure-source",
            Self::ConfigureKernel => "configure-kernel",
            Self::CompileKernel => "compile-kernel",
            Self::PackageOutput => "package-output",
            Self::Finalize => "finalize",
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
    pub fn run_build_verbose<G, C>(
        &self,
        config: &KernelBuildConfig,
        emit: G,
        is_cancelled: C,
        verbose: bool,
    ) -> Result<(), BuildError>
    where
        G: FnMut(String),
        C: Fn() -> bool,
    {
        self.run_build_internal(config, emit, is_cancelled, verbose)
    }

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
        emit: G,
        is_cancelled: C,
    ) -> Result<(), BuildError>
    where
        G: FnMut(String),
        C: Fn() -> bool,
    {
        self.run_build_internal(config, emit, is_cancelled, false)
    }

    fn run_build_internal<G, C>(
        &self,
        config: &KernelBuildConfig,
        mut emit: G,
        is_cancelled: C,
        verbose: bool,
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
        let mut packaged_artifacts: Vec<PathBuf> = Vec::new();

        let stages = [
            BuildStage::PrepareWorkspace,
            BuildStage::ValidateToolchain,
            BuildStage::EnsureSource,
            BuildStage::ConfigureKernel,
            BuildStage::CompileKernel,
            BuildStage::PackageOutput,
            BuildStage::Finalize,
        ];
        let total_stages = stages.len();

        for (index, stage) in stages.into_iter().enumerate() {
            if is_cancelled() {
                emit("\n--- Build Stopped by User ---\n".to_string());
                self.filesystem.write_string(
                    &paths.status_path,
                    "Build cancelled by user.\n",
                )?;
                return Ok(());
            }

            emit(format!(
                "__PROGRESS__|{}|{}|{}\n",
                index + 1,
                total_stages,
                stage.key()
            ));

            emit(stage.label().to_string());

            let stage_result = match stage {
                BuildStage::PrepareWorkspace => self.prepare_workspace(config, &paths),
                BuildStage::ValidateToolchain => self.ensure_toolchain(),
                BuildStage::EnsureSource => {
                    self.ensure_source_tree(config, &paths, &mut emit, verbose)
                }
                BuildStage::ConfigureKernel => {
                    self.configure_kernel(config, &paths, &mut emit, verbose)
                }
                BuildStage::CompileKernel => {
                    self.compile_kernel(config, &paths, &mut emit, verbose)
                }
                BuildStage::PackageOutput => match self.package_output(config, &paths, &mut emit, verbose) {
                    Ok(artifacts) => {
                        packaged_artifacts = artifacts;
                        Ok(())
                    }
                    Err(err) => Err(err),
                },
                BuildStage::Finalize => self.finalize_build(&paths),
            };

            if let Err(err) = stage_result {
                emit(format!("ERROR: Stage '{}' failed.\n", stage.key()));
                emit(format!("ERROR: Detail: {err}\n"));
                emit(format!(
                    "ERROR: Reference written to {}\n",
                    paths.status_path.display()
                ));
                self.filesystem.write_string(
                    &paths.status_path,
                    &format!("Build failed at stage {}: {err}\n", stage.key()),
                )?;
                return Err(err);
            }

            thread::sleep(Duration::from_millis(150));
        }

        let uname = self.run_shell("uname -r")?;
        if !uname.success {
            error!("uname command failed: {}", uname.stderr.trim());
            self.filesystem.write_string(
                &paths.status_path,
                &format!("Build failed at finalize step: {}\n", uname.stderr.trim()),
            )?;
            return Err(BuildError::CommandFailed(uname.stderr));
        }

        emit(format!("Host kernel: {}\n", uname.stdout.trim()));
        emit(format!("Manifest written to: {}\n", paths.manifest_path.display()));
        emit(format!("Snapshot written to: {}\n", paths.snapshot_path.display()));
        if config.package_format() == &PackageFormat::Debian && !packaged_artifacts.is_empty() {
            emit("Debian packages ready: kernel + headers verified.\n".to_string());
            for artifact in &packaged_artifacts {
                emit(format!("Artifact generated at: {}\n", artifact.display()));
            }
        } else {
            emit(format!("Artifact generated at: {}\n", paths.output_path.display()));
        }
        emit("SUCCESS! Kernel build pipeline completed in Rust.\n".to_string());
        Ok(())
    }

    fn prepare_workspace(&self, config: &KernelBuildConfig, paths: &BuildPaths) -> Result<(), BuildError> {
        if self.filesystem.path_exists(&paths.root) {
            self.filesystem.remove_dir_all(&paths.root)?;
        }

        self.filesystem.create_dir_all(&paths.root)?;
        self.filesystem.create_dir_all(&paths.bundle_dir)?;
        self.filesystem.create_dir_all(&paths.package_root)?;
        self.filesystem.create_dir_all(&paths.source_dir)?;
        self.write_manifest(config, paths, &[])?;
        Ok(())
    }

    fn ensure_source_tree(
        &self,
        config: &KernelBuildConfig,
        paths: &BuildPaths,
        emit: &mut dyn FnMut(String),
        verbose: bool,
    ) -> Result<(), BuildError> {
        let version = config.kernel_version();

        let command = format!(
            r#"
            set -e
            if [ -f {source}/Makefile ]; then
                true
            else
                mkdir -p {source}
                VERSION="{version}"
                MAJOR=$(echo $VERSION | cut -d. -f1)
                TARBALL="linux-$VERSION.tar.xz"
                BASE_URL="https://cdn.kernel.org/pub/linux/kernel/v$MAJOR.x"

                if [ ! -f {root}/"$TARBALL" ]; then
                    wget -qc -O {root}/"$TARBALL" "$BASE_URL/$TARBALL" || curl -L -C - -o {root}/"$TARBALL" "$BASE_URL/$TARBALL"
                fi
                tar -xf {root}/"$TARBALL" -C {source} --strip-components=1
            fi
            
            cd {source}
            if [ ! -f .patch_applied ]; then
                SCHED="{scheduler}"
                MAJOR=$(echo "{version}" | cut -d. -f1)
                MID=$(echo "{version}" | cut -d. -f2)
                PATCH_BASE="https://raw.githubusercontent.com/cachyos/kernel-patches/master/$MAJOR.$MID"

                apply_patch() {{
                    local url="$1"
                    local file=$(basename "$url")
                    wget -qO "$file" "$url" || return 1
                    if [ -s "$file" ]; then
                        if patch -p1 --forward --batch --dry-run < "$file" 2>&1 | grep -q "Reversed (or previously applied)"; then
                            rm -f "$file"
                            return 0
                        elif patch -p1 --forward --batch < "$file" >/dev/null 2>&1; then
                            rm -f "$file"
                            return 0
                        fi
                    fi
                    rm -f "$file"
                    return 1
                }}

                try_candidates() {{
                    local old_ifs="$IFS"
                    IFS='|'
                    for cand in $1; do
                        IFS="$old_ifs"
                        if apply_patch "$cand"; then return 0; fi
                        IFS='|'
                    done
                    IFS="$old_ifs"
                    return 1
                }}
                
                # Apply base
                if [ "$MAJOR" -lt 6 ] || ([ "$MAJOR" -eq 6 ] && [ "$MID" -lt 18 ]); then
                    try_candidates "$PATCH_BASE/all/0001-cachyos-base-all.patch"
                fi

                # Apply scheduler
                case "$SCHED" in
                    bore|cachyos)
                        if [ "$MAJOR" -lt 6 ] || ([ "$MAJOR" -eq 6 ] && [ "$MID" -lt 18 ]); then
                            try_candidates "$PATCH_BASE/sched-dev/0001-bore-cachy.patch|$PATCH_BASE/sched/0001-bore-cachy.patch"
                        else
                            try_candidates "$PATCH_BASE/sched-dev/0001-bore.patch|$PATCH_BASE/sched/0001-bore.patch"
                        fi
                        ;;
                    bmq)
                        if [ "$MAJOR" -lt 6 ] || ([ "$MAJOR" -eq 6 ] && [ "$MID" -lt 18 ]); then
                            try_candidates "$PATCH_BASE/sched-dev/0001-prjc-cachy-lfbmq.patch|$PATCH_BASE/sched/0001-prjc-cachy.patch"
                        else
                            try_candidates "$PATCH_BASE/sched-dev/0001-prjc-lfbmq.patch|$PATCH_BASE/sched/0001-prjc.patch"
                        fi
                        ;;
                    rt)
                        try_candidates "$PATCH_BASE/misc/0001-rt-i915.patch"
                        ;;
                esac

                touch .patch_applied
            fi
            "#,
            root = shell_quote(paths.root.to_string_lossy()),
            source = shell_quote(paths.source_dir.to_string_lossy()),
            version = version,
            scheduler = config.scheduler(),
        );

        let output = self.run_shell(&command)?;
        self.emit_process_output("source tree", &output, emit, verbose);
        if !output.success {
            return Err(BuildError::CommandFailed(output.stderr));
        }

        Ok(())
    }

    fn configure_kernel(
        &self,
        config: &KernelBuildConfig,
        paths: &BuildPaths,
        emit: &mut dyn FnMut(String),
        verbose: bool,
    ) -> Result<(), BuildError> {
        let config_commands = self.kernel_config_commands(config);

        self.filesystem
            .write_string(&paths.config_fragment_path, &config_commands.join("\n"))?;

        let command = format!(
            r#"
            set -e
            cd {source}
            if [ ! -f .config ]; then
                wget -qO .config "https://raw.githubusercontent.com/CachyOS/linux-cachyos/master/linux-cachyos/config" || make defconfig
            fi
            {commands}
            make olddefconfig
            "#,
            source = shell_quote(paths.source_dir.to_string_lossy()),
            commands = config_commands.join("\n"),
        );

        let output = self.run_shell(&command)?;
        self.emit_process_output("kernel configuration", &output, emit, verbose);
        if !output.success {
            return Err(BuildError::CommandFailed(output.stderr));
        }

        Ok(())
    }

    fn compile_kernel(
        &self,
        config: &KernelBuildConfig,
        paths: &BuildPaths,
        emit: &mut dyn FnMut(String),
        verbose: bool,
    ) -> Result<(), BuildError> {
        let cpu_count = cpu_count_shell();
        match config.package_format() {
            PackageFormat::Debian => {
                let bindeb_command = format!(
                    r#"
                    set -e
                    cd {source}
                    make -j{cpu_count} bindeb-pkg LOCALVERSION=-vinmod KDEB_PKGVERSION=1.0
                    
                    if [ "{zfs_enabled}" = "true" ]; then
                        KERNEL_VERSION=$(make kernelversion)-vinmod
                        ARCH=$(dpkg --print-architecture)
                        ZFS_DIR="../zfs-$KERNEL_VERSION"
                        ZFS_PKG_DIR="../zfs-pkg-$KERNEL_VERSION"
                        
                        cd ..
                        if [ ! -d "zfs-$KERNEL_VERSION" ]; then
                            git clone https://github.com/openzfs/zfs.git --depth 1 "zfs-$KERNEL_VERSION"
                        fi
                        cd "zfs-$KERNEL_VERSION"
                        ./autogen.sh
                        ./configure --prefix=/usr --sysconfdir=/etc --sbindir=/usr/bin \
                            --libdir=/usr/lib --datadir=/usr/share --includedir=/usr/include \
                            --with-udevdir=/lib/udev --libexecdir=/usr/lib/zfs --with-config=kernel \
                            --with-linux="../linux-src"
                        make -j{cpu_count}
                        
                        cd ..
                        mkdir -p "$ZFS_PKG_DIR/DEBIAN"
                        mkdir -p "$ZFS_PKG_DIR/lib/modules/$KERNEL_VERSION/extra"
                        
                        cat > "$ZFS_PKG_DIR/DEBIAN/control" <<EOF
Package: zfs-$KERNEL_VERSION
Version: 1.0
Section: kernel
Priority: optional
Architecture: $ARCH
Maintainer: VinMod
Description: ZFS modules for $KERNEL_VERSION
EOF

                        install -m644 zfs-$KERNEL_VERSION/module/*.ko "$ZFS_PKG_DIR/lib/modules/$KERNEL_VERSION/extra"
                        find "$ZFS_PKG_DIR" -name '*.ko' -exec zstd --rm -10 {{}} +
                        fakeroot dpkg-deb --build "$ZFS_PKG_DIR" "zfs-$KERNEL_VERSION.deb"
                    fi
                    "#,
                    source = shell_quote(paths.source_dir.to_string_lossy()),
                    cpu_count = cpu_count,
                    zfs_enabled = config.system_optimizations().contains(&"zfs".to_string()),
                );

                let bindeb_output = self.run_shell(&bindeb_command)?;
                self.emit_process_output("kernel compilation (bindeb)", &bindeb_output, emit, verbose);

                if bindeb_output.success {
                    return Ok(());
                }

                if !is_debian_builddep_failure(&bindeb_output) {
                    return Err(BuildError::CommandFailed(bindeb_output.stderr));
                }

                info!(
                    "Debian build dependencies not satisfied for bindeb-pkg; build must provide kernel and headers packages"
                );

                Err(BuildError::CommandFailed(format!(
                    "Debian build dependencies are missing; bindeb-pkg must succeed to produce kernel and headers packages: {}",
                    bindeb_output.stderr.trim()
                )))
            }
            _ => {
                let command = format!(
                    r#"
                    set -e
                    cd {source}
                    make -j{cpu_count}
                    make modules_install INSTALL_MOD_PATH={bundle} >/dev/null 2>&1 || true
                    "#,
                    source = shell_quote(paths.source_dir.to_string_lossy()),
                    bundle = shell_quote(paths.package_root.to_string_lossy()),
                    cpu_count = cpu_count,
                );

                let output = self.run_shell(&command)?;
                self.emit_process_output("kernel compilation", &output, emit, verbose);
                if !output.success {
                    return Err(BuildError::CommandFailed(output.stderr));
                }

                Ok(())
            }
        }
    }

    fn package_output(
        &self,
        config: &KernelBuildConfig,
        paths: &BuildPaths,
        emit: &mut dyn FnMut(String),
        verbose: bool,
    ) -> Result<Vec<PathBuf>, BuildError> {
        match config.package_format() {
            PackageFormat::Debian => {
                let locate_command = format!(
                    r#"
                    set -e
                    find {parent} -maxdepth 1 -type f -name '*.deb' | sort
                    "#,
                    parent = shell_quote(paths.source_dir.parent().unwrap_or(&paths.root).to_string_lossy()),
                );

                let output = self.run_shell(&locate_command)?;
                self.emit_process_output("package discovery", &output, emit, verbose);
                if !output.success {
                    return Err(BuildError::CommandFailed(output.stderr));
                }

                let mut artifact_paths = Vec::new();
                for line in output.stdout.lines() {
                    let trimmed = line.trim();
                    if trimmed.is_empty() {
                        continue;
                    }

                    let file_name = PathBuf::from(trimmed)
                        .file_name()
                        .and_then(|name| name.to_str())
                        .unwrap_or_default()
                        .to_lowercase();

                    if file_name.contains("headers") || file_name.contains("image") || file_name.contains("kernel") || file_name.contains("zfs") {
                        artifact_paths.push(PathBuf::from(trimmed));
                    }
                }

                let kernel_artifact = artifact_paths
                    .iter()
                    .find(|path| {
                        let name = path.file_name().and_then(|name| name.to_str()).unwrap_or_default().to_lowercase();
                        name.contains("image") || name.contains("kernel")
                    })
                    .cloned();

                let headers_artifact = artifact_paths
                    .iter()
                    .find(|path| {
                        let name = path.file_name().and_then(|name| name.to_str()).unwrap_or_default().to_lowercase();
                        name.contains("headers")
                    })
                    .cloned();
                    
                let zfs_artifact = artifact_paths
                    .iter()
                    .find(|path| {
                        let name = path.file_name().and_then(|name| name.to_str()).unwrap_or_default().to_lowercase();
                        name.contains("zfs")
                    })
                    .cloned();

                let kernel_artifact = kernel_artifact.ok_or_else(|| {
                    BuildError::MissingArtifact("Debian build did not produce a kernel package".to_string())
                })?;

                let headers_artifact = headers_artifact.ok_or_else(|| {
                    BuildError::MissingArtifact("Debian build did not produce a headers package".to_string())
                })?;

                let mut produced = vec![kernel_artifact, headers_artifact];
                if let Some(zfs) = zfs_artifact {
                    produced.push(zfs);
                }
                self.write_manifest(config, paths, &produced)?;

                Ok(produced)
            }
            PackageFormat::RedHat | PackageFormat::Arch | PackageFormat::Tarball => {
                let command = format!(
                    r#"
                    set -e
                    tar -czf {archive} -C {root} \
                        'build-manifest.txt' \
                        'system-snapshot.txt' \
                        'kernel-profile.fragment' \
                        'bundle'
                    "#,
                    archive = shell_quote(paths.output_path.to_string_lossy()),
                    root = shell_quote(paths.root.to_string_lossy()),
                );

                let output = self.run_shell(&command)?;
                self.emit_process_output("artifact packaging", &output, emit, verbose);
                if !output.success {
                    return Err(BuildError::CommandFailed(output.stderr));
                }

                self.write_manifest(config, paths, &[paths.output_path.clone()])?;
                Ok(vec![paths.output_path.clone()])
            }
        }
    }

    fn finalize_build(&self, paths: &BuildPaths) -> Result<(), BuildError> {
        self.filesystem.write_string(
            &paths.status_path,
            "Build completed successfully.\n",
        )?;
        Ok(())
    }

    fn ensure_toolchain(&self) -> Result<(), BuildError> {
        let mut requirements = vec!["wget", "curl", "gcc", "make", "tar", "bc", "flex", "bison", "perl"];
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

        commands.push("scripts/config -e CACHYOS".to_string());
        
        let arch_string = config.architecture().to_uppercase();
        let march = if arch_string == "NATIVE" {
            let gcc_out = self.run_shell("gcc -Q -march=native --help=target | grep -m1 march= | awk '{print toupper($2)}'")
                .map(|p| p.stdout)
                .unwrap_or_default();
            let mut detected = gcc_out.trim().to_string();
            if detected.is_empty() { detected = "GENERIC".to_string() }
            let mapped = match detected.as_str() {
                "ZNVER1" => "MZEN".to_string(),
                "ZNVER2" => "MZEN2".to_string(),
                "ZNVER3" => "MZEN3".to_string(),
                "ZNVER4" => "MZEN4".to_string(),
                "BDVER1" => "MBULLDOZER".to_string(),
                "BDVER2" => "MPILEDRIVER".to_string(),
                "BDVER3" => "MSTEAMROLLER".to_string(),
                "BDVER4" => "MEXCAVATOR".to_string(),
                "BTVER1" => "MBOBCAT".to_string(),
                "BTVER2" => "MJAGUAR".to_string(),
                "AMDFAM10" => "MMK10".to_string(),
                "K8-SSE3" => "MK8SSE3".to_string(),
                "BONNELL" => "MATOM".to_string(),
                "GOLDMONT-PLUS" => "MGOLDMONTPLUS".to_string(),
                "SKYLAKE-AVX512" => "MSKYLAKEX".to_string(),
                "ICELAKE-CLIENT" => "MICELAKE".to_string(),
                _ => format!("M{}", detected),
            };
            mapped
        } else {
            match arch_string.as_str() {
                "ZEN4" => "MZEN4".to_string(),
                "ZEN3" => "MZEN3".to_string(),
                "SKYLAKE" => "MSKYLAKEX".to_string(),
                _ => "GENERIC".to_string()
            }
        };

        if march != "GENERIC" {
            commands.push("scripts/config -k --disable CONFIG_GENERIC_CPU".to_string());
            commands.push(format!("scripts/config -k --enable {}", format!("CONFIG_{}", march)));
        }

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
        if config.system_optimizations().contains(&"performance_governor".to_string()) {
            commands.push("scripts/config -e CPU_FREQ_GOV_PERFORMANCE -e CPU_FREQ_DEFAULT_GOV_PERFORMANCE".to_string());
        } else {
            commands.push("scripts/config -e CPU_FREQ_GOV_SCHEDUTIL -e CPU_FREQ_DEFAULT_GOV_SCHEDUTIL".to_string());
        }
        
        commands.push("scripts/config -d CC_OPTIMIZE_FOR_PERFORMANCE -d CC_OPTIMIZE_FOR_SIZE".to_string());
        if config.system_optimizations().contains(&"O3".to_string()) {
            commands.push("scripts/config -e CC_OPTIMIZE_FOR_PERFORMANCE_O3".to_string());
        } else if config.system_optimizations().contains(&"Os".to_string()) {
            commands.push("scripts/config -e CC_OPTIMIZE_FOR_SIZE".to_string());
        } else {
            commands.push("scripts/config -e CC_OPTIMIZE_FOR_PERFORMANCE".to_string());
        }

        commands.push("scripts/config -e NET -e INET -e TCP_CONG_ADVANCED -e NET_SCH_FQ".to_string());
        commands.push("scripts/config -d TCP_CONG_BBR -d TCP_CONG_BBR3 -d DEFAULT_RENO -d DEFAULT_CUBIC -d DEFAULT_BBR -d DEFAULT_BBR3".to_string());
        if config.system_optimizations().contains(&"tcp_bbr3".to_string()) {
            commands.push("scripts/config -e TCP_CONG_BBR3 -e DEFAULT_BBR3 --set-str DEFAULT_TCP_CONG bbr3".to_string());
        } else {
            commands.push("scripts/config -e DEFAULT_CUBIC --set-str DEFAULT_TCP_CONG cubic".to_string());
        }

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

    fn write_manifest(
        &self,
        config: &KernelBuildConfig,
        paths: &BuildPaths,
        artifacts: &[PathBuf],
    ) -> Result<(), BuildError> {
        let artifact_list = if artifacts.is_empty() {
            String::from("[]")
        } else {
            let rendered = artifacts
                .iter()
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>()
                .join(",");
            format!("[{rendered}]")
        };

        let manifest = format!(
            "kernel_version={}\narchitecture={}\nscheduler={}\nlto={}\nhz={}\nnr_cpus={}\ntick_type={}\npreemption={}\npackage={}\noptimizations={:?}\nroot={}\nsource_dir={}\noutputs={}\n",
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
            artifact_list,
        );

        self.filesystem.write_string(&paths.manifest_path, &manifest)?;
        Ok(())
    }

    fn run_shell(&self, command: &str) -> Result<ProcessOutput, BuildError> {
        Ok(self.process.run("sh", &["-lc", command])?)
    }

    fn emit_process_output(
        &self,
        label: &str,
        output: &ProcessOutput,
        emit: &mut dyn FnMut(String),
        verbose: bool,
    ) {
        if !output.stdout.trim().is_empty() {
            info!("{} stdout: {}", label, output.stdout.trim());
            if verbose {
                emit(format!("[{}] stdout:\n{}\n", label, output.stdout.trim_end()));
            }
        }
        if !output.stderr.trim().is_empty() {
            info!("{} stderr: {}", label, output.stderr.trim());
            if verbose {
                emit(format!("[{}] stderr:\n{}\n", label, output.stderr.trim_end()));
            }
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

fn is_debian_builddep_failure(output: &ProcessOutput) -> bool {
    let text = format!("{}\n{}", output.stdout, output.stderr).to_lowercase();
    text.contains("dpkg-checkbuilddeps")
        || text.contains("unmet build dependencies")
        || text.contains("build dependencies/conflicts unsatisfied")
}