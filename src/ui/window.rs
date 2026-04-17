use gtk4::Stack;
use gtk4::StackTransitionType;
use libadwaita::prelude::*;
use libadwaita::{Application, ApplicationWindow, HeaderBar, NavigationPage, NavigationSplitView, ToolbarView};

use crate::ui::form::BuildForm;

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

    let stack_sidebar = gtk4::StackSidebar::builder().stack(&view_stack).build();
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

    view_stack.add_titled(&crate::ui::form::build_kernel_page(&form), Some("kernel"), "Kernel & CPU");
    view_stack.add_titled(&crate::ui::form::build_options_page(&form), Some("options"), "Tuning & Memory");
    view_stack.add_titled(&crate::ui::form::build_console_page(&form), Some("console"), "Console");

    window.set_content(Some(&split_view));
    window.present();
}