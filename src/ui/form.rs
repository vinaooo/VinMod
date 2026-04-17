use std::cell::Cell;
use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;

use gtk4::{Button, StringList, Stack, StackSidebar, StackTransitionType};
use libadwaita::prelude::*;
use libadwaita::{
    Application, ApplicationWindow, ComboRow, HeaderBar, NavigationPage, NavigationSplitView,
    PreferencesGroup, PreferencesPage, SpinRow, SwitchRow, ToolbarView,
};

use crate::core::builder::{
    CpuScheduler, KernelBuildConfig, LtoMode, PackageFormat, PreemptionType, TickType,
};
use crate::core::service::DefaultBuildService;

pub struct BuildForm {
    kernel_version_row: ComboRow,
    architecture_row: ComboRow,
    scheduler_row: ComboRow,
    lto_row: ComboRow,
    tick_rate_row: ComboRow,
    tick_type_row: ComboRow,
    preempt_row: ComboRow,
    hugepages_row: ComboRow,
    package_format_row: ComboRow,
    cpus_row: SpinRow,
    o3_switch: SwitchRow,
    os_switch: SwitchRow,
    governor_switch: SwitchRow,
    bbr3_switch: SwitchRow,
    zfs_switch: SwitchRow,
}

impl BuildForm {
    pub fn new() -> Self {
        Self {
            kernel_version_row: create_labeled_dropdown(
                "Kernel Version",
                &[
                    ("6.19.12 (Recomendada)", "A versão atual estabilizada com os patches CachyOS. Ideal para o usuário padrão."),
                    ("Série 7.x (Mainline)", "O código mais recente do Kernel Upstream Linux. Pode conter instabilidades novas."),
                    ("Custom...", "Define commits ou branchs especificas para propósitos de teste profundo."),
                ],
            ),
            architecture_row: create_labeled_dropdown(
                "CPU Architecture",
                &[
                    ("native", "Flags automáticas (gcc -march=native) habilitando exclusividades dinâmicas da sua CPU real."),
                    ("zen4", "Prioriza os pipelines da arquitetura AMD Zen 4 (Ryzen 7000+)."),
                    ("zen3", "Ajusta saltos de pipeline e caches focando arquitetura AMD Zen 3 (Ryzen 5000)."),
                    ("skylake", "Otimiza a compilação inteiramente em diretrizes IPC famosas da Intel Skylake+."),
                    ("x86-64-v3", "Baseline genérico contendo suporte fixo a AVX2."),
                    ("x86-64-v4", "Baseline focado em vetorizações matemáticas agressivas 512bits (AVX-512)."),
                    ("custom", "Insere scripts e CFLAGS 100% customizadas sob seus termos."),
                ],
            ),
            scheduler_row: create_labeled_dropdown(
                "CPU Scheduler",
                &[
                    ("cachyos (BORE)", "Recomendado! Altíssima priorização em resposta de frame em jogos massivos."),
                    ("bore", "Burst-Oriented Response Enhancer padrão. Perfeito para ambiente desktop em alto uso."),
                    ("eevdf", "Agendador oficial padrão balanceado do upstream do kernel linux (substituto do CFS)."),
                    ("bmq", "BitMap Queue, escalonamento por bitmap priorizando latência puramente técnica inter-processos."),
                    ("rt", "Real-Time (Preempt RT). Patches usados no meio profissional para zero áudio/vídeo drops."),
                    ("rt-bore", "Real-time aplicado ao topo do BORE (Exótico, alto consumo de cpu)."),
                    ("hardened", "Patches focados exclusivamente em mitigação teórica contra exploits de canais laterais."),
                    ("none", "Mantém escolhas de fallback legadas focadas em estabilidade massiva (pouca responsividade)."),
                ],
            ),
            lto_row: create_labeled_dropdown(
                "LLVM LTO",
                &[
                    ("thin", "Otimiza linkagens de forma global no build, excelente balanço entre tempo e velocidade."),
                    ("thin-dist", "Perfil thin especial para gerar os mirrors de compilações universais do CachyOS."),
                    ("full (Heavy RAM)", "Máxima LTO global. Consume ~2 a 3GiB de RAM por núcleo lógico, use com cuidado!"),
                    ("none", "Linkagem tradicional e bruta focando unicamente em compilação super rápida."),
                ],
            ),
            tick_rate_row: create_labeled_dropdown(
                "Tick Rate (HZ)",
                &[
                    ("1000 Hz", "O máximo de polling desktop, precisão microscópica do mouse sacrificando mais interrupts."),
                    ("750 Hz", "Ótima taxa responsiva intermediária sem impactar laptops baseados em baterias fracas/calor."),
                    ("600 Hz", "Balanço comum, excelente para tarefas fluidas de multimidia em desktop médio."),
                    ("500 Hz", "Exato milissegundo dobrado. Reduz latência a 2ms úteis por timer core."),
                    ("300 Hz", "Recomendação clássica do kernel LTS para desktops não focados no 1% de low latency."),
                    ("250 Hz", "Usado grandemente no Debian, foca throughput de redes massivas sem muita percepção local."),
                    ("100 Hz", "Foco absurdo em Troughput/Servidor Web ignorando reações de UI para zero interrupções."),
                ],
            ),
            tick_type_row: create_labeled_dropdown(
                "Tick Type",
                &[
                    ("nohz_idle", "O ticket clássico que desliga temporizadores em CPUs idle para evitar aquecimento e drain de energia."),
                    ("nohz_full", "Eliminação completa dos ticks temporizados nas CPUs ativas, diminuindo máxima latência de processos core."),
                    ("periodic", "Apenas dispara os callbacks em frequências de ciclo fechadas. Útil a hardware arcaico/bugado."),
                ],
            ),
            preempt_row: create_labeled_dropdown(
                "Preempt Type",
                &[
                    ("preempt (Preemptible)", "Force-preemption das chaves do sistema para o processo do jogo priorizado."),
                    ("voluntary", "A preempção só acorda voluntariamente, focado pesado em Servidor Web / File."),
                    ("preempt_dynamic", "Modo moderno que injeta patches de preempção selecionáveis durante boot (Grub)."),
                    ("none", "Máquina nua e calculista com foco apenas no processamento puro (servidores longos)."),
                ],
            ),
            hugepages_row: create_labeled_dropdown(
                "Hugepages Support",
                &[
                    ("madvise (Safest/General)", "Safe Default: Fornece RAM em grandes blocos apenas quando programas (MADVISE) solicitam."),
                    ("always", "Agrega nativamente para RAM páginas massivas em 100% da memória (MUITO propenso a Leaks em 8GB, cuidado!)"),
                    ("none", "Mantém a granularidade normal e bloqueia páginas Transparentes contra fragmentações estritas."),
                ],
            ),
            package_format_row: create_labeled_dropdown(
                "Package Format",
                &[
                    ("Debian (.deb)", "Gera pacote para sistemas baseados em Debian/Ubuntu (padrão atual)."),
                    ("Red Hat (.rpm)", "Gera pacote para Fedora, Rocky, AlmaLinux, etc."),
                    ("Arch Linux (pkg.tar.zst)", "Gera pacote nativo para Arch Linux, Manjaro, CachyOS."),
                    ("Tarball (.tar.gz)", "Gera um arquivo unificado simples bruto."),
                ],
            ),
            cpus_row: create_labeled_spinbutton(
                "Max CPU/Thread Capacity (NR_CPUS)",
                1.0,
                1024.0,
                std::thread::available_parallelism().map(|n| n.get()).unwrap_or(16) as f64,
                Some("Para a máxima performance absoluta (minimal overhead em jogos), coloque exatamente o número de threads físicos/lógicos do seu processador."),
            ),
            o3_switch: create_checkbox("Enable O3 Optimization (-O3 compiler flags)", true),
            os_switch: create_checkbox("Enable OS Optimization (-Os size flags)", false),
            governor_switch: create_checkbox("Enable Performance CPU Governor by default", true),
            bbr3_switch: create_checkbox("Enable TCP BBR3 Congestion Control", true),
            zfs_switch: create_checkbox("Enable ZFS Support", false),
        }
    }
}

pub fn build_main_window(app: &Application) {
    let window = ApplicationWindow::builder()
        .application(app)
        .title("VinMod")
        .default_width(900)
        .default_height(700)
        .build();

    let view_stack = Stack::builder()
        .transition_type(StackTransitionType::Crossfade)
        .build();

    let content_toolbar = ToolbarView::builder().build();
    let header_bar = HeaderBar::builder().build();
    content_toolbar.add_top_bar(&header_bar);
    content_toolbar.set_content(Some(&view_stack));

    let content_page = NavigationPage::builder()
        .child(&content_toolbar)
        .title("Content")
        .build();

    let stack_sidebar = StackSidebar::builder().stack(&view_stack).build();
    let sidebar_toolbar = ToolbarView::builder().content(&stack_sidebar).build();
    sidebar_toolbar.add_top_bar(&HeaderBar::builder().build());

    let sidebar_page = NavigationPage::builder()
        .child(&sidebar_toolbar)
        .title("Menu")
        .build();

    let split_view = NavigationSplitView::builder()
        .sidebar(&sidebar_page)
        .content(&content_page)
        .build();

    let form = BuildForm::new();
    let page_kernel = build_kernel_page(&form);
    view_stack.add_titled(&page_kernel, Some("kernel"), "Kernel & CPU");

    let page_options = build_options_page(&form);
    view_stack.add_titled(&page_options, Some("options"), "Tuning & Memory");

    let page_console = build_console_page(&form);
    view_stack.add_titled(&page_console, Some("console"), "Console");

    window.set_content(Some(&split_view));
    window.present();
}

fn create_labeled_dropdown(title: &str, items: &[(&str, &str)]) -> ComboRow {
    let options: Vec<&str> = items.iter().map(|(opt, _)| *opt).collect();
    let string_list = StringList::new(&options);

    let row = ComboRow::builder()
        .title(title)
        .model(&string_list)
        .use_subtitle(false)
        .build();

    let explanations: Vec<String> = items.iter().map(|(_, exp)| exp.to_string()).collect();

    if let Some(first_exp) = explanations.first() {
        if !first_exp.is_empty() {
            row.set_subtitle(first_exp);
        }
    }

    let explanations_clone = explanations.clone();
    row.connect_selected_notify(move |combo_row| {
        let idx = combo_row.selected() as usize;
        if let Some(exp) = explanations_clone.get(idx) {
            combo_row.set_subtitle(if exp.is_empty() { "" } else { exp });
        }
    });

    row
}

fn create_labeled_spinbutton(title: &str, min: f64, max: f64, default: f64, tooltip: Option<&str>) -> SpinRow {
    let adj = gtk4::Adjustment::new(default, min, max, 1.0, 10.0, 0.0);
    let row = SpinRow::builder()
        .title(title)
        .adjustment(&adj)
        .numeric(true)
        .build();

    if let Some(text) = tooltip {
        row.set_subtitle(text);
    }

    row
}

fn create_checkbox(title: &str, default_active: bool) -> SwitchRow {
    SwitchRow::builder()
        .title(title)
        .active(default_active)
        .build()
}

fn kernel_version_from_index(index: u32) -> &'static str {
    match index {
        1 => "7.x-mainline",
        2 => "custom",
        _ => "6.19.12",
    }
}

fn architecture_from_index(index: u32) -> &'static str {
    match index {
        1 => "zen4",
        2 => "zen3",
        3 => "skylake",
        4 => "x86-64-v3",
        5 => "x86-64-v4",
        6 => "custom",
        _ => "native",
    }
}

fn scheduler_from_index(index: u32) -> CpuScheduler {
    match index {
        1 => CpuScheduler::Bore,
        2 => CpuScheduler::Eevdf,
        3 => CpuScheduler::Bmq,
        4 => CpuScheduler::RealTime,
        _ => CpuScheduler::CachyOsBore,
    }
}

fn lto_from_index(index: u32) -> LtoMode {
    match index {
        1 => LtoMode::ThinDist,
        2 => LtoMode::Full,
        3 => LtoMode::None,
        _ => LtoMode::Thin,
    }
}

fn tick_type_from_index(index: u32) -> TickType {
    match index {
        1 => TickType::NoHzFull,
        2 => TickType::Periodic,
        _ => TickType::NoHzIdle,
    }
}

fn preemption_from_index(index: u32) -> PreemptionType {
    match index {
        1 => PreemptionType::Voluntary,
        2 => PreemptionType::PreemptDynamic,
        3 => PreemptionType::None,
        _ => PreemptionType::Preempt,
    }
}

fn selected_optimizations(
    o3_enabled: bool,
    os_enabled: bool,
    governor_enabled: bool,
    bbr3_enabled: bool,
    zfs_enabled: bool,
) -> Vec<String> {
    let mut items = Vec::new();

    if o3_enabled {
        items.push("O3".to_string());
    }
    if os_enabled {
        items.push("Os".to_string());
    }
    if governor_enabled {
        items.push("performance_governor".to_string());
    }
    if bbr3_enabled {
        items.push("tcp_bbr3".to_string());
    }
    if zfs_enabled {
        items.push("zfs".to_string());
    }

    items
}

fn hugepages_from_index(index: u32) -> &'static str {
    match index {
        1 => "always",
        2 => "none",
        _ => "madvise",
    }
}

fn status_stage_from_message(message: &str) -> Option<&'static str> {
    match message.trim() {
        "Preparing build workspace..." => Some("Preparing workspace"),
        "Validating toolchain and system requirements..." => Some("Validating toolchain"),
        "Acquiring kernel source tree..." => Some("Acquiring source tree"),
        "Configuring kernel profile..." => Some("Configuring kernel"),
        "Compiling kernel and modules..." => Some("Compiling kernel"),
        "Packaging build output..." => Some("Packaging output"),
        "Finalizing build output..." => Some("Finalizing"),
        _ => None,
    }
}

fn stage_label_from_key(key: &str) -> &'static str {
    match key {
        "prepare-workspace" => "Preparing workspace",
        "validate-toolchain" => "Validating toolchain",
        "ensure-source" => "Acquiring source tree",
        "configure-kernel" => "Configuring kernel",
        "compile-kernel" => "Compiling kernel",
        "package-output" => "Packaging output",
        "finalize" => "Finalizing",
        _ => "Running",
    }
}

pub fn build_kernel_page(form: &BuildForm) -> PreferencesPage {
    let page = PreferencesPage::new();

    let group = PreferencesGroup::builder()
        .title("Kernel & CPU Architecture")
        .build();
    page.add(&group);

    group.add(&form.kernel_version_row);
    group.add(&form.architecture_row);
    group.add(&form.scheduler_row);

    page
}

pub fn build_options_page(form: &BuildForm) -> PreferencesPage {
    let page = PreferencesPage::new();

    let group1 = PreferencesGroup::builder()
        .title("Advanced Tuning & Latency")
        .build();
    page.add(&group1);

    group1.add(&form.lto_row);
    group1.add(&form.tick_rate_row);
    group1.add(&form.tick_type_row);

    let group2 = PreferencesGroup::builder()
        .title("Memory & Optimizations")
        .build();
    page.add(&group2);

    group2.add(&form.cpus_row);
    group2.add(&form.preempt_row);
    group2.add(&form.hugepages_row);
    group2.add(&form.o3_switch);
    group2.add(&form.os_switch);
    group2.add(&form.governor_switch);
    group2.add(&form.bbr3_switch);
    group2.add(&form.zfs_switch);

    page
}

pub fn build_console_page(form: &BuildForm) -> gtk4::Box {
    let container = gtk4::Box::new(gtk4::Orientation::Vertical, 0);

    let page = PreferencesPage::new();

    let pkg_group = PreferencesGroup::builder()
        .title("Packaging Options")
        .build();
    page.add(&pkg_group);

    pkg_group.add(&form.package_format_row);

    let action_group = PreferencesGroup::builder()
        .title("Build Action")
        .build();
    page.add(&action_group);

    let action_row = libadwaita::ActionRow::builder()
        .title("Start Process")
        .subtitle("Click below to start the compilation process.")
        .activatable(true)
        .build();

    let text_buffer = gtk4::TextBuffer::new(None);
    let text_view = gtk4::TextView::builder()
        .editable(false)
        .monospace(true)
        .buffer(&text_buffer)
        .build();

    let status_label = gtk4::Label::new(Some("Status: Idle"));
    status_label.set_halign(gtk4::Align::Start);
    status_label.set_margin_start(12);
    status_label.set_margin_end(12);
    status_label.set_margin_top(8);
    status_label.set_margin_bottom(8);

    let progress_bar = gtk4::ProgressBar::new();
    progress_bar.set_fraction(0.0);
    progress_bar.set_show_text(true);
    progress_bar.set_text(Some("0/0 - Idle"));
    progress_bar.set_margin_start(12);
    progress_bar.set_margin_end(12);
    progress_bar.set_margin_bottom(8);

    let text_buffer_clone = text_buffer.clone();
    let package_format_row_clone = form.package_format_row.clone();
    let kernel_version_row_clone = form.kernel_version_row.clone();
    let architecture_row_clone = form.architecture_row.clone();
    let scheduler_row_clone = form.scheduler_row.clone();
    let lto_row_clone = form.lto_row.clone();
    let tick_rate_row_clone = form.tick_rate_row.clone();
    let tick_type_row_clone = form.tick_type_row.clone();
    let preempt_row_clone = form.preempt_row.clone();
    let hugepages_row_clone = form.hugepages_row.clone();
    let cpus_row_clone = form.cpus_row.clone();
    let o3_switch_clone = form.o3_switch.clone();
    let os_switch_clone = form.os_switch.clone();
    let governor_switch_clone = form.governor_switch.clone();
    let bbr3_switch_clone = form.bbr3_switch.clone();
    let zfs_switch_clone = form.zfs_switch.clone();
    let status_label_clone = status_label.clone();
    let progress_bar_clone = progress_bar.clone();

    let is_building = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let is_building_clone = is_building.clone();
    let current_stage = Rc::new(RefCell::new(String::from("Starting")));

    let launch_build: Rc<dyn Fn(Button, bool)> = {
        let is_building_clone = is_building_clone.clone();
        let status_label_clone = status_label_clone.clone();
        let progress_bar_clone = progress_bar_clone.clone();
        let current_stage = current_stage.clone();
        let text_buffer_clone = text_buffer_clone.clone();
        let package_format_row_clone = package_format_row_clone.clone();
        let kernel_version_row_clone = kernel_version_row_clone.clone();
        let architecture_row_clone = architecture_row_clone.clone();
        let scheduler_row_clone = scheduler_row_clone.clone();
        let lto_row_clone = lto_row_clone.clone();
        let tick_rate_row_clone = tick_rate_row_clone.clone();
        let tick_type_row_clone = tick_type_row_clone.clone();
        let preempt_row_clone = preempt_row_clone.clone();
        let hugepages_row_clone = hugepages_row_clone.clone();
        let cpus_row_clone = cpus_row_clone.clone();
        let o3_switch_clone = o3_switch_clone.clone();
        let os_switch_clone = os_switch_clone.clone();
        let governor_switch_clone = governor_switch_clone.clone();
        let bbr3_switch_clone = bbr3_switch_clone.clone();
        let zfs_switch_clone = zfs_switch_clone.clone();

        Rc::new(move |btn: Button, previous_build_exists: bool| {
            is_building_clone.store(true, std::sync::atomic::Ordering::SeqCst);
            btn.set_label("Stop Build Process");
            status_label_clone.set_label("Status: Starting...");
            progress_bar_clone.set_fraction(0.0);
            progress_bar_clone.set_text(Some("0/7 - Starting"));
            current_stage.replace("Starting".to_string());

            log::info!("Build process initiated from UI");
            let buffer = text_buffer_clone.clone();
            buffer.set_text("Starting build process...\n");
            let mut start_iter = buffer.end_iter();
            buffer.insert(
                &mut start_iter,
                "Build can take several minutes depending on hardware and selected options.\n",
            );
            if previous_build_exists {
                status_label_clone.set_label("Status: Cleaning previous build...");
                progress_bar_clone.set_text(Some("0/7 - Cleaning previous build"));
                let mut cleanup_iter = buffer.end_iter();
                buffer.insert(
                    &mut cleanup_iter,
                    "Previous build artifacts detected and will be deleted before this run.\n",
                );
            }

            let (sender, receiver) = std::sync::mpsc::channel::<String>();
            let btn_clone = btn.clone();
            let main_context_buffer = buffer.clone();
            let main_context_btn = btn_clone.clone();
            let status_label_main = status_label_clone.clone();
            let progress_bar_main = progress_bar_clone.clone();
            let current_stage_main = current_stage.clone();

            gtk4::glib::source::timeout_add_local(std::time::Duration::from_millis(50), move || {
                let mut finished = false;
                while let Ok(text) = receiver.try_recv() {
                    if text == "__FINISHED__" {
                        main_context_btn.set_label("Start Build Process");
                        status_label_main.set_label("Status: Completed");
                        progress_bar_main.set_fraction(1.0);
                        progress_bar_main.set_text(Some("Completed"));
                        finished = true;
                    } else if text.starts_with("ERROR:") {
                        status_label_main.set_label("Status: Error");
                        progress_bar_main.set_text(Some("Error"));
                    } else if text.starts_with("__PROGRESS__|") {
                        let payload = text.trim();
                        let mut parts = payload.split('|');
                        let _marker = parts.next();
                        let current = parts.next().and_then(|v| v.parse::<usize>().ok());
                        let total = parts.next().and_then(|v| v.parse::<usize>().ok());
                        let stage_key = parts.next().unwrap_or("running");

                        if let (Some(current), Some(total)) = (current, total) {
                            if total > 0 {
                                progress_bar_main.set_fraction(current as f64 / total as f64);
                                progress_bar_main.set_text(Some(&format!(
                                    "{current}/{total} - {}",
                                    stage_label_from_key(stage_key)
                                )));
                            }
                        }

                        current_stage_main.replace(stage_label_from_key(stage_key).to_string());
                        status_label_main.set_label(&format!(
                            "Status: {}",
                            stage_label_from_key(stage_key)
                        ));
                    } else {
                        if let Some(stage) = status_stage_from_message(&text) {
                            current_stage_main.replace(stage.to_string());
                            status_label_main.set_label(&format!("Status: {stage}"));
                        }
                        let mut end_iter = main_context_buffer.end_iter();
                        main_context_buffer.insert(&mut end_iter, &text);
                    }
                }

                if finished {
                    gtk4::glib::ControlFlow::Break
                } else {
                    gtk4::glib::ControlFlow::Continue
                }
            });

            let heartbeat_seconds = Rc::new(Cell::new(0u64));
            let heartbeat_seconds_clone = heartbeat_seconds.clone();
            let heartbeat_buffer = buffer.clone();
            let heartbeat_flag = is_building_clone.clone();
            let heartbeat_stage = current_stage.clone();
            let heartbeat_status = status_label_clone.clone();
            let heartbeat_progress = progress_bar_clone.clone();

            gtk4::glib::source::timeout_add_local(std::time::Duration::from_secs(5), move || {
                if !heartbeat_flag.load(std::sync::atomic::Ordering::SeqCst) {
                    return gtk4::glib::ControlFlow::Break;
                }

                let elapsed = heartbeat_seconds_clone.get() + 5;
                heartbeat_seconds_clone.set(elapsed);

                let spinner = match (elapsed / 5) % 4 {
                    0 => "|",
                    1 => "/",
                    2 => "-",
                    _ => "\\",
                };

                let mut end_iter = heartbeat_buffer.end_iter();
                heartbeat_buffer.insert(
                    &mut end_iter,
                    &format!("{spinner} Build still running... elapsed: {elapsed}s\n"),
                );

                let stage = heartbeat_stage.borrow().clone();
                heartbeat_status.set_label(&format!("Status: {stage} ({elapsed}s elapsed)"));
                heartbeat_progress.pulse();

                gtk4::glib::ControlFlow::Continue
            });

            let cancel_flag = is_building_clone.clone();
            let package_format = PackageFormat::from_index(package_format_row_clone.selected());
            let kernel_version = kernel_version_row_clone.selected();
            let architecture = architecture_row_clone.selected();
            let scheduler = scheduler_row_clone.selected();
            let lto = lto_row_clone.selected();
            let hz = tick_rate_row_clone.selected();
            let nr_cpus = cpus_row_clone.value() as u32;
            let tick_type = tick_type_row_clone.selected();
            let preemption = preempt_row_clone.selected();
            let hugepages = hugepages_row_clone.selected();
            let o3_enabled = o3_switch_clone.is_active();
            let os_enabled = os_switch_clone.is_active();
            let governor_enabled = governor_switch_clone.is_active();
            let bbr3_enabled = bbr3_switch_clone.is_active();
            let zfs_enabled = zfs_switch_clone.is_active();

            std::thread::spawn(move || {
                let config = KernelBuildConfig::new()
                    .with_kernel_version(kernel_version_from_index(kernel_version))
                    .with_architecture(architecture_from_index(architecture))
                    .with_scheduler(scheduler_from_index(scheduler))
                    .with_lto(lto_from_index(lto))
                    .with_hz(match hz {
                        1 => 750,
                        2 => 600,
                        3 => 500,
                        4 => 300,
                        5 => 250,
                        6 => 100,
                        _ => 1000,
                    })
                    .with_nr_cpus(nr_cpus)
                    .with_tick_type(tick_type_from_index(tick_type))
                    .with_preemption(preemption_from_index(preemption))
                    .with_package_format(package_format)
                    .with_system_optimizations(selected_optimizations(
                        o3_enabled,
                        os_enabled,
                        governor_enabled,
                        bbr3_enabled,
                        zfs_enabled,
                    ));

                let _hugepages_profile = hugepages_from_index(hugepages);
                let service = DefaultBuildService::new();

                let run_result = service.run_build(
                    &config,
                    |message| {
                        let _ = sender.send(message);
                    },
                    || !cancel_flag.load(std::sync::atomic::Ordering::SeqCst),
                );

                if let Err(err) = run_result {
                    let _ = sender.send(format!("ERROR: {err}\n"));
                }

                cancel_flag.store(false, std::sync::atomic::Ordering::SeqCst);
                let _ = sender.send("__FINISHED__".to_string());
            });
        })
    };

    let build_btn = Button::with_label("Start Build Process");
    build_btn.set_valign(gtk4::Align::Center);
    build_btn.connect_clicked(move |btn| {
        if is_building_clone.load(std::sync::atomic::Ordering::SeqCst) {
            is_building_clone.store(false, std::sync::atomic::Ordering::SeqCst);
            btn.set_label("Start Build Process");
            status_label_clone.set_label("Status: Stopping...");
            progress_bar_clone.set_text(Some("Stopping..."));
            log::info!("Build process stopped from UI");
            return;
        }

        let previous_build_exists = Path::new("build-artifacts").exists();
        if previous_build_exists {
            if let Some(root) = btn.root() {
                if let Ok(window) = root.downcast::<gtk4::Window>() {
                    let dialog = gtk4::MessageDialog::builder()
                        .transient_for(&window)
                        .modal(true)
                        .message_type(gtk4::MessageType::Question)
                        .buttons(gtk4::ButtonsType::OkCancel)
                        .text("Previous build detected")
                        .secondary_text("VinMod will remove the previous build artifacts before starting this new build. Continue?")
                        .build();
                    let launch_build = launch_build.clone();
                    let btn_for_start = btn.clone();
                    let status_label = status_label_clone.clone();
                    let progress_bar = progress_bar_clone.clone();
                    dialog.connect_response(move |dialog, response| {
                        if response == gtk4::ResponseType::Ok {
                            launch_build(btn_for_start.clone(), true);
                        } else {
                            status_label.set_label("Status: Idle");
                            progress_bar.set_fraction(0.0);
                            progress_bar.set_text(Some("0/0 - Idle"));
                        }
                        dialog.close();
                    });
                    dialog.present();
                    return;
                }
            }
        }
        launch_build(btn.clone(), false);
    });

    action_row.add_suffix(&build_btn);
    action_group.add(&action_row);
    container.append(&page);
    container.append(&status_label);
    container.append(&progress_bar);

    let scrolled_window = gtk4::ScrolledWindow::builder()
        .child(&text_view)
        .vexpand(true)
        .min_content_height(300)
        .build();

    container.append(&scrolled_window);
    container
}