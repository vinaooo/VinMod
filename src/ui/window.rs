use gtk4::{Button, StringList, Stack, StackSidebar, StackTransitionType};
use libadwaita::prelude::*;
use libadwaita::{
    Application, ApplicationWindow, ComboRow, HeaderBar, NavigationPage, NavigationSplitView,
    PreferencesGroup, PreferencesPage, SpinRow, SwitchRow, ToolbarView,
};

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
    
    // In libadwaita 1.4+ HeaderBar.show_title(false) is default and the property sometimes isn't exposed like we tried. So we just build a HeaderBar.
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

    let page_kernel = build_kernel_page();
    view_stack.add_titled(&page_kernel, Some("kernel"), "Kernel & CPU");

    let page_options = build_options_page();
    view_stack.add_titled(&page_options, Some("options"), "Tuning & Memory");

    let page_console = build_console_page();
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

    // Set the initial subtitle for the default selected item (index 0)
    if let Some(first_exp) = explanations.first() {
        if !first_exp.is_empty() {
            row.set_subtitle(first_exp);
        }
    }

    // Update the subtitle dynamically when the user selects a new option
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

fn build_kernel_page() -> PreferencesPage {
    let page = PreferencesPage::new();

    let group = PreferencesGroup::builder()
        .title("Kernel & CPU Architecture")
        .build();
    page.add(&group);

    group.add(&create_labeled_dropdown(
        "Kernel Version",
        &[
            ("6.19.12 (Recomendada)", "A versão atual estabilizada com os patches CachyOS. Ideal para o usuário padrão."),
            ("Série 7.x (Mainline)", "O código mais recente do Kernel Upstream Linux. Pode conter instabilidades novas."),
            ("Custom...", "Define commits ou branchs especificas para propósitos de teste profundo.")
        ],
    ));

    group.add(&create_labeled_dropdown(
        "CPU Architecture",
        &[
            ("native", "Flags automáticas (gcc -march=native) habilitando exclusividades dinâmicas da sua CPU real."),
            ("zen4", "Prioriza os pipelines da arquitetura AMD Zen 4 (Ryzen 7000+)."),
            ("zen3", "Ajusta saltos de pipeline e caches focando arquitetura AMD Zen 3 (Ryzen 5000)."),
            ("skylake", "Otimiza a compilação inteiramente em diretrizes IPC famosas da Intel Skylake+."),
            ("x86-64-v3", "Baseline genérico contendo suporte fixo a AVX2."),
            ("x86-64-v4", "Baseline focado em vetorizações matemáticas agressivas 512bits (AVX-512)."),
            ("custom", "Insere scripts e CFLAGS 100% customizadas sob seus termos.")
        ],
    ));

    group.add(&create_labeled_dropdown(
        "CPU Scheduler",
        &[
            ("cachyos (BORE)", "Recomendado! Altíssima priorização em resposta de frame em jogos massivos."),
            ("bore", "Burst-Oriented Response Enhancer padrão. Perfeito para ambiente desktop em alto uso."),
            ("eevdf", "Agendador oficial padrão balanceado do upstream do kernel linux (substituto do CFS)."),
            ("bmq", "BitMap Queue, escalonamento por bitmap priorizando latência puramente técnica inter-processos."),
            ("rt", "Real-Time (Preempt RT). Patches usados no meio profissional para zero áudio/vídeo drops."),
            ("rt-bore", "Real-time aplicado ao topo do BORE (Exótico, alto consumo de cpu)."),
            ("hardened", "Patches focados exclusivamente em mitigação teórica contra exploits de canais laterais."),
            ("none", "Mantém escolhas de fallback legadas focadas em estabilidade massiva (pouca responsividade).")
        ],
    ));

    page
}

fn build_options_page() -> PreferencesPage {
    let page = PreferencesPage::new();

    let group1 = PreferencesGroup::builder()
        .title("Advanced Tuning & Latency")
        .build();
    page.add(&group1);

    group1.add(&create_labeled_dropdown(
        "LLVM LTO",
        &[
            ("thin", "Otimiza linkagens de forma global no build, excelente balanço entre tempo e velocidade."),
            ("thin-dist", "Perfil thin especial para gerar os mirrors de compilações universais do CachyOS."),
            ("full (Heavy RAM)", "Máxima LTO global. Consume ~2 a 3GiB de RAM por núcleo lógico, use com cuidado!"),
            ("none", "Linkagem tradicional e bruta focando unicamente em compilação super rápida.")
        ],
    ));

    group1.add(&create_labeled_dropdown(
        "Tick Rate (HZ)",
        &[
            ("1000 Hz", "O máximo de polling desktop, precisão microscópica do mouse sacrificando mais interrupts."),
            ("750 Hz", "Ótima taxa responsiva intermediária sem impactar laptops baseados em baterias fracas/calor."),
            ("600 Hz", "Balanço comum, excelente para tarefas fluidas de multimidia em desktop médio."),
            ("500 Hz", "Exato milissegundo dobrado. Reduz latência a 2ms úteis por timer core."),
            ("300 Hz", "Recomendação clássica do kernel LTS para desktops não focados no 1% de low latency."),
            ("250 Hz", "Usado grandemente no Debian, foca throughput de redes massivas sem muita percepção local."),
            ("100 Hz", "Foco absurdo em Troughput/Servidor Web ignorando reações de UI para zero interrupções.")
        ],
    ));

    group1.add(&create_labeled_dropdown(
        "Tick Type",
        &[
            ("nohz_idle", "O ticket clássico que desliga temporizadores em CPUs idle para evitar aquecimento e drain de energia."),
            ("nohz_full", "Eliminação completa dos ticks temporizados nas CPUs ativas, diminuindo máxima latência de processos core."),
            ("periodic", "Apenas dispara os callbacks em frequências de ciclo fechadas. Útil a hardware arcaico/bugado.")
        ],
    ));

    let group2 = PreferencesGroup::builder()
        .title("Memory & Optimizations")
        .build();
    page.add(&group2);

    let logical_cores = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(16) as f64;
    let recommended_nr_cpus = logical_cores; // Maximum performance = exact logic core count

    group2.add(&create_labeled_spinbutton(
        "Max CPU/Thread Capacity (NR_CPUS)",
        1.0,
        1024.0,
        recommended_nr_cpus,
        Some("Para a máxima performance absoluta (minimal overhead em jogos), coloque exatamente o número de threads físicos/lógicos do seu processador."),
    ));

    group2.add(&create_labeled_dropdown(
        "Preempt Type",
        &[
            ("preempt (Preemptible)", "Force-preemption das chaves do sistema para o processo do jogo priorizado."),
            ("voluntary", "A preempção só acorda voluntariamente, focado pesado em Servidor Web / File."),
            ("preempt_dynamic", "Modo moderno que injeta patches de preempção selecionáveis durante boot (Grub)."),
            ("none", "Máquina nua e calculista com foco apenas no processamento puro (servidores longos).")
        ],
    ));

    group2.add(&create_labeled_dropdown(
        "Hugepages Support",
        &[
            ("madvise (Safest/General)", "Safe Default: Fornece RAM em grandes blocos apenas quando programas (MADVISE) solicitam."),
            ("always", "Agrega nativamente para RAM páginas massivas em 100% da memória (MUITO propenso a Leaks em 8GB, cuidado!)"),
            ("none", "Mantém a granularidade normal e bloqueia páginas Transparentes contra fragmentações estritas.")
        ],
    ));

    group2.add(&create_checkbox("Enable O3 Optimization (-O3 compiler flags)", true));
    group2.add(&create_checkbox("Enable OS Optimization (-Os size flags)", false));
    group2.add(&create_checkbox("Enable Performance CPU Governor by default", true));
    group2.add(&create_checkbox("Enable TCP BBR3 Congestion Control", true));
    group2.add(&create_checkbox("Enable ZFS Support", false));

    page
}

fn build_console_page() -> gtk4::Box {
    let container = gtk4::Box::new(gtk4::Orientation::Vertical, 0);

    let page = PreferencesPage::new();

    let pkg_group = PreferencesGroup::builder()
        .title("Packaging Options")
        .build();
    page.add(&pkg_group);

    pkg_group.add(&create_labeled_dropdown(
        "Package Format",
        &[
            ("Debian (.deb)", "Gera pacote para sistemas baseados em Debian/Ubuntu (padrão atual)."),
            ("Red Hat (.rpm)", "Gera pacote para Fedora, Rocky, AlmaLinux, etc."),
            ("Arch Linux (pkg.tar.zst)", "Gera pacote nativo para Arch Linux, Manjaro, CachyOS."),
            ("Tarball (.tar.gz)", "Gera um arquivo unificado simples bruto.")
        ],
    ));

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

    let text_buffer_clone = text_buffer.clone();
    
    let is_building = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let is_building_clone = is_building.clone();

    let build_btn = Button::with_label("Start Build Process");
    build_btn.set_valign(gtk4::Align::Center);
    build_btn.connect_clicked(move |btn| {
        if is_building_clone.load(std::sync::atomic::Ordering::SeqCst) {
            is_building_clone.store(false, std::sync::atomic::Ordering::SeqCst);
            btn.set_label("Start Build Process");
            log::info!("Build process stopped from UI");
            let mut end_iter = text_buffer_clone.end_iter();
            text_buffer_clone.insert(&mut end_iter, "\n--- Build Stopped by User ---\n");
            return;
        }

        is_building_clone.store(true, std::sync::atomic::Ordering::SeqCst);
        btn.set_label("Stop Build Process");
        
        log::info!("Build process initiated from UI");
        let buffer = text_buffer_clone.clone();
        
        // Limpa o buffer anterior
        buffer.set_text("Starting build process...\n");
        
        let (sender, receiver) = gtk4::glib::MainContext::channel(gtk4::glib::Priority::DEFAULT);
        let btn_clone = btn.clone();
        
        receiver.attach(
            None,
            move |text: String| {
                if text == "__FINISHED__" {
                    btn_clone.set_label("Start Build Process");
                } else {
                    let mut end_iter = buffer.end_iter();
                    buffer.insert(&mut end_iter, &text);
                }
                gtk4::glib::ControlFlow::Continue
            }
        );
        
        let cancel_flag = is_building_clone.clone();
        std::thread::spawn(move || {
            let steps = [
                ("Reading configuration...\n", 500),
                ("Validating dependencies...\n", 800),
                ("Applying CachyOS and BORE patches...\n", 1200),
                ("Compiling kernel (mock/placeholder)...\n", 1500),
                ("Generating package...\n", 1000),
                ("SUCCESS! Build finished.\n", 0),
            ];
            
            for (msg, delay) in steps {
                if !cancel_flag.load(std::sync::atomic::Ordering::SeqCst) {
                    break;
                }
                let _ = sender.send(msg.to_string());
                if delay > 0 {
                    // Verificação de interrupção a cada 100ms
                    let intervals = delay / 100;
                    for _ in 0..intervals {
                        if !cancel_flag.load(std::sync::atomic::Ordering::SeqCst) {
                            break;
                        }
                        std::thread::sleep(std::time::Duration::from_millis(100));
                    }
                }
            }
            cancel_flag.store(false, std::sync::atomic::Ordering::SeqCst);
            let _ = sender.send("__FINISHED__".to_string());
        });
    });

    action_row.add_suffix(&build_btn);

    action_group.add(&action_row);

    container.append(&page);

    let scrolled_window = gtk4::ScrolledWindow::builder()
        .child(&text_view)
        .vexpand(true)
        .min_content_height(300)
        .build();

    container.append(&scrolled_window);

    container
}

