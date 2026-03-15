//! Leptos UI for Polymarket copy-trading bot. Fetches /api/state and displays logs, dashboard, settings, positions.
//! Real-time updates: EventSource subscribes to /api/state/stream (SSE); backend pushes when new activity is logged.

use leptos::*;
use leptos_router::*;
use serde::Deserialize;
use wasm_bindgen::JsCast;
use web_sys::EventSource;

#[derive(Clone, Debug, Default, Deserialize)]
pub struct TradeLog {
    pub time: String,
    pub tag: String,
    pub side: String,
    pub outcome: String,
    pub size: String,
    pub price: String,
    pub slug: String,
    pub target_address: Option<String>,
    pub copy_status: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct PositionSummary {
    pub slug: String,
    pub outcome: String,
    pub size: f64,
    pub cur_price: f64,
    pub delta: Option<f64>,
    pub delta_at: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct Status {
    pub mode: String,
    pub targets: u32,
    pub wallet: Option<String>,
    pub target_addresses: Option<Vec<String>>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct UiConfig {
    pub delta_highlight_sec: u64,
    pub delta_animation_sec: u64,
}

#[derive(Clone, Debug, Default, Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LeaderboardEntry {
    pub rank: Option<String>,
    pub proxy_wallet: Option<String>,
    pub user_name: Option<String>,
    pub vol: Option<f64>,
    pub pnl: Option<f64>,
    pub profile_image: Option<String>,
    pub x_username: Option<String>,
    pub verified_badge: Option<bool>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct BotState {
    pub logs: Vec<TradeLog>,
    pub status: Status,
    pub positions: std::collections::HashMap<String, Vec<PositionSummary>>,
    pub ui: UiConfig,
}

async fn fetch_state() -> Result<BotState, String> {
    let url = format!("/api/state?t={}", js_sys::Date::now() as u64);
    gloo_net::http::Request::get(&url)
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())
}

async fn fetch_leaderboard(
    category: &str,
    time_period: &str,
    order_by: &str,
) -> Result<Vec<LeaderboardEntry>, String> {
    let url = format!(
        "/api/leaderboard?category={}&timePeriod={}&orderBy={}&limit=50",
        urlencoding_encode(category),
        urlencoding_encode(time_period),
        urlencoding_encode(order_by),
    );
    gloo_net::http::Request::get(&url)
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())
}

const TARGET_COLORS_KEY: &str = "target_colors";
const THEME_KEY: &str = "theme";

fn load_theme() -> String {
    if let Some(window) = web_sys::window() {
        if let Ok(Some(storage)) = window.local_storage() {
            if let Ok(Some(t)) = storage.get_item(THEME_KEY) {
                if t == "light" || t == "dark" {
                    return t;
                }
            }
        }
    }
    "dark".to_string()
}

fn save_theme(theme: &str) {
    if let Some(window) = web_sys::window() {
        if let Ok(Some(storage)) = window.local_storage() {
            let _ = storage.set_item(THEME_KEY, theme);
        }
    }
}

fn random_hex_color() -> String {
    let r = (js_sys::Math::random() * 256.0) as u8;
    let g = (js_sys::Math::random() * 256.0) as u8;
    let b = (js_sys::Math::random() * 256.0) as u8;
    format!("#{:02x}{:02x}{:02x}", r, g, b)
}

fn load_target_colors_from_storage() -> std::collections::HashMap<String, String> {
    let window = match web_sys::window() {
        Some(w) => w,
        None => return std::collections::HashMap::new(),
    };
    let storage = match window.local_storage() {
        Ok(Some(s)) => s,
        _ => return std::collections::HashMap::new(),
    };
    match storage.get_item(TARGET_COLORS_KEY) {
        Ok(Some(s)) => serde_json::from_str(&s).unwrap_or_default(),
        _ => std::collections::HashMap::new(),
    }
}

fn save_target_colors_to_storage(map: &std::collections::HashMap<String, String>) {
    if let Some(window) = web_sys::window() {
        if let Ok(Some(storage)) = window.local_storage() {
            let _ = storage.set_item(TARGET_COLORS_KEY, &serde_json::to_string(map).unwrap_or_else(|_| "{}".to_string()));
        }
    }
}

fn urlencoding_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b'.' {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{:02X}", b));
        }
    }
    out
}

#[component]
fn Layout(
    nav: impl IntoView + 'static,
    header: impl IntoView + 'static,
    main: impl IntoView + 'static,
    #[prop(optional)] aside: Option<impl IntoView + 'static>,
    theme: RwSignal<String>,
    sidebar_open: RwSignal<bool>,
) -> impl IntoView {
    let theme_attr = move || theme.get();
    view! {
        <div
            class="app-shell"
            class:sidebar-open=move || sidebar_open.get()
            data-theme=theme_attr
        >
            <div
                class="sidebar-overlay"
                on:click=move |_| sidebar_open.set(false)
            ></div>
            {nav}
            <div class="main-content flex flex-col flex-1 min-w-0 overflow-hidden">
                <header class="shrink-0 flex items-center gap-3 mb-4">{header}</header>
                <div class="flex flex-1 min-h-0 overflow-hidden gap-0 flex-col">
                    {match aside {
                        Some(a) => view! { <div class="flex flex-1 min-h-0 gap-4"><div class="flex-1 min-w-0 overflow-hidden flex flex-col">{main}</div><aside class="w-full md:w-[380px] shrink-0 overflow-hidden flex flex-col">{a}</aside></div> }.into_view(),
                        None => view! { <div class="flex-1 min-w-0 overflow-hidden flex flex-col">{main}</div> }.into_view(),
                    }}
                </div>
            </div>
        </div>
    }
}

#[component]
fn Sidebar(
    theme: RwSignal<String>,
    sidebar_open: RwSignal<bool>,
) -> impl IntoView {
    let location = use_location();
    let path = move || location.pathname.get();
    let is_active = move |p: &str| {
        let binding = path().trim_end_matches('/').to_string();
        let trim = p.trim_end_matches('/');
        (binding.is_empty() && trim == "/") || binding == trim
    };
    let dashboard_icon = "<svg xmlns='http://www.w3.org/2000/svg' width='20' height='20' viewBox='0 0 24 24' fill='none' stroke='currentColor' stroke-width='2' stroke-linecap='round' stroke-linejoin='round'><rect x='3' y='3' width='7' height='7'/><rect x='14' y='3' width='7' height='7'/><rect x='14' y='14' width='7' height='7'/><rect x='3' y='14' width='7' height='7'/></svg>";
    let agent_icon = "<svg xmlns='http://www.w3.org/2000/svg' width='20' height='20' viewBox='0 0 24 24' fill='none' stroke='currentColor' stroke-width='2'><circle cx='12' cy='12' r='3'/><path d='M12 1v2M12 21v2M4.22 4.22l1.42 1.42M18.36 18.36l1.42 1.42M1 12h2M21 12h2M4.22 19.78l1.42-1.42M18.36 5.64l1.42-1.42'/></svg>";
    let logs_icon = "<svg xmlns='http://www.w3.org/2000/svg' width='20' height='20' viewBox='0 0 24 24' fill='none' stroke='currentColor' stroke-width='2'><path d='M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z'/><polyline points='14 2 14 8 20 8'/><line x1='16' y1='13' x2='8' y2='13'/><line x1='16' y1='17' x2='8' y2='17'/><polyline points='10 9 9 9 8 9'/></svg>";
    let traders_icon = "<svg xmlns='http://www.w3.org/2000/svg' width='20' height='20' viewBox='0 0 24 24' fill='none' stroke='currentColor' stroke-width='2'><path d='M17 21v-2a4 4 0 0 0-4-4H5a4 4 0 0 0-4 4v2'/><circle cx='9' cy='7' r='4'/><path d='M23 21v-2a4 4 0 0 0-3-3.87M16 3.13a4 4 0 0 1 0 7.75'/></svg>";
    let portfolio_icon = "<svg xmlns='http://www.w3.org/2000/svg' width='20' height='20' viewBox='0 0 24 24' fill='none' stroke='currentColor' stroke-width='2'><line x1='12' y1='20' x2='12' y2='10'/><line x1='18' y1='20' x2='18' y2='4'/><line x1='6' y1='20' x2='6' y2='16'/></svg>";
    let theme_icon = "<svg xmlns='http://www.w3.org/2000/svg' width='20' height='20' viewBox='0 0 24 24' fill='none' stroke='currentColor' stroke-width='2'><path d='M21 12.79A9 9 0 1 1 11.21 3 7 7 0 0 0 21 12.79z'/></svg>";
    let settings_icon = "<svg xmlns='http://www.w3.org/2000/svg' width='20' height='20' viewBox='0 0 24 24' fill='none' stroke='currentColor' stroke-width='2'><circle cx='12' cy='12' r='3'/><path d='M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 0 1 0 2.83 2 2 0 0 1-2.83 0l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-2 2 2 2 0 0 1-2-2v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 0 1-2.83 0 2 2 0 0 1 0-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1-2-2 2 2 0 0 1 2-2h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 0 1 0-2.83 2 2 0 0 1 2.83 0l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 2-2 2 2 0 0 1 2 2v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 0 1 2.83 0 2 2 0 0 1 0 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 2 2 2 2 0 0 1-2 2h-.09a1.65 1.65 0 0 0-1.51 1z'/></svg>";
    view! {
        <nav class="sidebar">
            <div class="sidebar-top">
                <div class="nav-label">"Navigation"</div>
                <A href="/" class=move || if is_active("/") { "active" } else { "" } on:click=move |_| sidebar_open.set(false)>
                    <span class="sidebar-icon" inner_html=dashboard_icon></span>
                    <span>"Dashboard"</span>
                </A>
                <A href="/agent" class=move || if is_active("/agent") { "active" } else { "" } on:click=move |_| sidebar_open.set(false)>
                    <span class="sidebar-icon" inner_html=agent_icon></span>
                    <span>"Agent"</span>
                </A>
                <A href="/logs" class=move || if is_active("/logs") { "active" } else { "" } on:click=move |_| sidebar_open.set(false)>
                    <span class="sidebar-icon" inner_html=logs_icon></span>
                    <span>"Logs"</span>
                </A>
                <A href="/toptraders" class=move || if is_active("/toptraders") { "active" } else { "" } on:click=move |_| sidebar_open.set(false)>
                    <span class="sidebar-icon" inner_html=traders_icon></span>
                    <span>"Top Traders"</span>
                </A>
                <A href="/portfolio" class=move || if is_active("/portfolio") { "active" } else { "" } on:click=move |_| sidebar_open.set(false)>
                    <span class="sidebar-icon" inner_html=portfolio_icon></span>
                    <span>"Portfolio"</span>
                </A>
            </div>
            <div class="sidebar-bottom">
                <div class="nav-label">"Preferences"</div>
                <button
                    type="button"
                    class="flex items-center gap-2 w-full px-4 py-2 mx-2 rounded-md text-left text-sm border-none cursor-pointer"
                    style="background: transparent; color: var(--text-muted);"
                    on:click=move |_| {
                        let next = if theme.get() == "dark" { "light" } else { "dark" };
                        theme.set(next.to_string());
                        save_theme(&next);
                    }
                >
                    <span class="sidebar-icon" inner_html=theme_icon></span>
                    <span>{move || if theme.get() == "dark" { "Light theme" } else { "Dark theme" }}</span>
                </button>
                <A href="/settings" class=move || if is_active("/settings") { "active" } else { "" } on:click=move |_| sidebar_open.set(false)>
                    <span class="sidebar-icon" inner_html=settings_icon></span>
                    <span>"Settings"</span>
                </A>
            </div>
        </nav>
    }
}

#[component]
fn LogPage(
    logs: impl Fn() -> Vec<TradeLog> + 'static,
    selected_target: impl Fn() -> Option<String> + 'static,
    target_colors: RwSignal<std::collections::HashMap<String, String>>,
    loading: impl Fn() -> bool + 'static,
) -> impl IntoView {
    view! {
        {move || {
            let _ = target_colors.get();
            let all_logs = logs();
            let filtered = match selected_target() {
                None => all_logs,
                Some(ref addr) => all_logs
                    .into_iter()
                    .filter(|r| {
                        r.target_address
                            .as_ref()
                            .map(|a| a.eq_ignore_ascii_case(addr))
                            .unwrap_or(false)
                    })
                    .collect(),
            };
            let rows: Vec<_> = filtered.into_iter().rev().collect();
            let is_loading = loading();
            view! {
                <div class="flex-1 overflow-auto overflow-x-auto min-h-0 flex flex-col p-4">
                    {if is_loading {
                        view! {
                            <p class="text-sm text-muted">"Loading activities…"</p>
                        }.into_view()
                    } else {
                        view! {
                            <p class="section-label mb-2">
                                "Showing " {rows.len()} " buy/sell activities (newest first)."
                            </p>
                            {if rows.is_empty() {
                                view! {
                                    <p class="text-sm text-muted py-4">"No activities yet. Trades from your target addresses will appear here."</p>
                                }.into_view()
                            } else {
                                view! {
                                    <table class="w-full border-collapse text-xs">
                        <thead>
                            <tr>
                                <th class="p-2 text-left text-muted font-medium border-b border">"Time"</th>
                                <th class="p-2 text-left text-muted font-medium border-b border">"Tag"</th>
                                <th class="p-2 text-left text-muted font-medium border-b border">"Side"</th>
                                <th class="p-2 text-left text-muted font-medium border-b border">"Outcome"</th>
                                <th class="p-2 text-left text-muted font-medium border-b border">"Size"</th>
                                <th class="p-2 text-left text-muted font-medium border-b border">"Price"</th>
                                <th class="p-2 text-left text-muted font-medium border-b border">"Market"</th>
                                <th class="p-2 text-left text-muted font-medium border-b border">"Target"</th>
                                <th class="p-2 text-left text-muted font-medium border-b border">"Status"</th>
                            </tr>
                        </thead>
                        <tbody>
                            {rows
                        .into_iter()
                        .enumerate()
                        .map(|(i, r)| {
                            let time_short = if r.time.len() >= 19 {
                                r.time[11..19].to_string()
                            } else {
                                r.time.clone()
                            };
                            let side_class = if r.side.eq_ignore_ascii_case("BUY") {
                                "side-buy"
                            } else if r.side.eq_ignore_ascii_case("SELL") {
                                "side-sell"
                            } else {
                                ""
                            };
                            let r_time = r.time.clone();
                            let r_tag = r.tag.clone();
                            let r_side = r.side.clone();
                            let r_outcome = r.outcome.clone();
                            let r_size = r.size.clone();
                            let r_price = r.price.clone();
                            let price_display = r_price
                                .parse::<f64>()
                                .map(|n| format!("{:.3}", n))
                                .unwrap_or(r_price.clone());
                            let r_slug = r.slug.clone();
                            let r_target = r.target_address.clone();
                            let r_status = r.copy_status.clone();
                            let colors = target_colors.get();
                            let cell_color = r_target.as_ref().and_then(|a| colors.get(a).cloned()).unwrap_or_else(|| "#8af".to_string());
                            view! {
                                <tr key=format!("{:?}-{}", r_time, i) class="border-b border">
                                    <td class="p-2">{time_short}</td>
                                    <td class="p-2">{r_tag}</td>
                                    <td class=format!("p-2 {}", side_class)>{r_side}</td>
                                    <td class="p-2">{r_outcome}</td>
                                    <td class="p-2 tabular-nums">
                                        {if let Ok(n) = r_size.parse::<f64>() {
                                            format!("{:.2}", n)
                                        } else {
                                            r_size
                                        }}
                                    </td>
                                    <td class="p-2 tabular-nums">{price_display}</td>
                                    <td class="p-2 break-words">{r_slug}</td>
                                    <td class="p-2 font-mono text-[11px] break-all" style=format!("color: {}", cell_color)>
                                        {r_target.unwrap_or_default()}
                                    </td>
                                    <td class="p-2">{r_status.unwrap_or_default()}</td>
                                </tr>
                            }
                        })
                        .collect_view()}
                        </tbody>
                    </table>
                                }.into_view()
                            }}
                        }.into_view()
                    }}
                </div>
            }
        }}
    }
}

#[component]
fn AgentPage() -> impl IntoView {
    view! {
        <div class="flex-1 overflow-auto p-4">
            <h1 class="page-title">"Agent"</h1>
            <p class="page-desc mb-4">"Autonomous market intelligence and analysis."</p>
            <div class="card p-4 max-w-2xl">
                <p class="text-sm text-muted">
                    "Connect your exchange in Portfolio and the agent will pre-load positions and wallet, then research signals, sentiment, catalysts, and risk. Ask for market overview or risk check."
                </p>
                <div class="mt-4 flex flex-wrap gap-2">
                    <button class="btn btn-primary">"Research current market"</button>
                    <button class="btn btn-secondary">"Full analysis of my book"</button>
                    <button class="btn btn-secondary">"Check risk and exposure"</button>
                </div>
            </div>
        </div>
    }
}

#[component]
fn PortfolioPage(state: Option<BotState>) -> impl IntoView {
    let positions = state
        .as_ref()
        .map(|s| s.positions.clone())
        .unwrap_or_default();
    let flat: Vec<_> = positions
        .into_iter()
        .flat_map(|(slug, list)| list.into_iter().map(move |p| (slug.clone(), p)))
        .collect();
    let total_value = flat.iter().fold(0.0_f64, |acc, (_, p)| acc + p.size * p.cur_price);
    view! {
        <div class="flex-1 overflow-auto p-4">
            <h1 class="page-title">"Portfolio"</h1>
            <p class="page-desc mb-4">"Positions and allocation."</p>
            <div class="grid gap-3 md:grid-cols-3 mb-6">
                <div class="card p-4">
                    <span class="section-label">"TOTAL VALUE"</span>
                    <p class="text-xl font-semibold tabular-nums">{format!("${:.2}", total_value)}</p>
                </div>
                <div class="card p-4">
                    <span class="section-label">"OPEN POSITIONS"</span>
                    <p class="text-xl font-semibold tabular-nums">{flat.len()}</p>
                </div>
                <div class="card p-4">
                    <span class="section-label">"AVAILABLE"</span>
                    <p class="text-muted text-sm">"—"</p>
                </div>
            </div>
            <div class="card p-4">
                <span class="section-label">"ACTIVE POSITIONS"</span>
                {if flat.is_empty() {
                    view! {
                        <p class="text-muted text-sm py-4">"No open positions — Open your first position to start trading."</p>
                    }.into_view()
                } else {
                    view! {
                        <ul class="divide-y divide-border mt-2">
                            {flat
                                .into_iter()
                                .map(|(slug, p)| {
                                    view! {
                                        <li class="py-2 flex justify-between items-center gap-2">
                                            <span class="font-mono text-sm">{slug}</span>
                                            <span class="text-muted text-sm">{p.outcome}</span>
                                            <span class="tabular-nums">{format!("{:.4}", p.size)} " @ " {format!("{:.4}", p.cur_price)}</span>
                                        </li>
                                    }
                                })
                                .collect_view()}
                        </ul>
                    }.into_view()
                }}
            </div>
        </div>
    }
}

#[component]
fn DashboardPage(state: Option<BotState>) -> impl IntoView {
    let mode = state.as_ref().map(|s| s.status.mode.clone()).unwrap_or_else(|| "—".to_string());
    let targets = state.as_ref().map(|s| s.status.targets).unwrap_or(0);
    let addresses = state.as_ref().and_then(|s| s.status.target_addresses.clone()).unwrap_or_default();
    let recent: Vec<TradeLog> = state
        .as_ref()
        .map(|s| s.logs.iter().rev().take(5).cloned().collect())
        .unwrap_or_default();
    view! {
        <div class="flex-1 overflow-auto p-4">
            <h1 class="page-title">"Dashboard"</h1>
            <p class="text-sm mb-4">"Overview and current status."</p>
            <div class="grid gap-3 max-w-md">
                <div class="card p-3">
                    <span class="section-label">"Mode"</span>
                    <p class="text-sm">{mode.clone()}</p>
                </div>
                <div class="card p-3">
                    <span class="section-label">"Targets"</span>
                    <p class="text-sm">{targets} " target(s)"</p>
                    {if !addresses.is_empty() {
                        view! {
                            <div class="text-dim text-xs mt-1 font-mono break-all space-y-0.5">
                                {addresses
                                    .into_iter()
                                    .map(|addr| view! { <p class="break-all">{addr}</p> })
                                    .collect_view()}
                            </div>
                        }
                            .into_view()
                    } else {
                        view! {}.into_view()
                    }}
                </div>
                <div class="card p-3">
                    <span class="section-label">"Recent activity"</span>
                    {if recent.is_empty() {
                        view! { <p class="text-muted text-sm">"No activity yet."</p> }.into_view()
                    } else {
                        view! {
                            <ul class="text-sm mt-1 space-y-1">
                                {recent
                                    .into_iter()
                    .map(|r| {
                        let t = if r.time.len() >= 19 {
                            r.time[11..19].to_string()
                        } else {
                            r.time.clone()
                        };
                        let s = r.side.clone();
                        let o = r.outcome.clone();
                        let p = r.price.clone();
                        let sl = r.slug.clone();
                        view! {
                            <li>
                                {t} " " {s} " " {o} " @ " {p} " — " {sl}
                            </li>
                        }
                    })
                                    .collect_view()}
                            </ul>
                        }
                            .into_view()
                    }}
                </div>
            </div>
        </div>
    }
}

#[component]
fn SettingsPage(
    state: Option<BotState>,
    target_colors: RwSignal<std::collections::HashMap<String, String>>,
) -> impl IntoView {
    let mode = state.as_ref().map(|s| s.status.mode.clone()).unwrap_or_else(|| "—".to_string());
    let targets = state.as_ref().map(|s| s.status.targets).unwrap_or(0);
    let addresses = state
        .as_ref()
        .and_then(|s| s.status.target_addresses.clone())
        .unwrap_or_default();
    let wallet = state
        .as_ref()
        .and_then(|s| s.status.wallet.clone())
        .unwrap_or_else(|| "—".to_string());
    let default_ui = UiConfig::default();
    let ui = state.as_ref().map(|s| s.ui.clone()).unwrap_or(default_ui);
    view! {
        <div class="flex-1 overflow-auto p-4">
            <h1 class="page-title">"Settings"</h1>
            <p class="text-sm mb-4">"Current bot configuration. Target colors apply to the Log page."</p>
            <div class="flex flex-col gap-3 max-w-md">
                <div class="card p-3 flex items-center justify-between gap-2">
                    <span class="text-sm text-muted">"Mode"</span>
                    <span class="text-xs font-medium">{mode}</span>
                </div>
                <div class="card p-3 flex items-center justify-between gap-2">
                    <span class="text-sm text-muted">"Targets"</span>
                    <span class="text-xs tabular-nums">{targets}</span>
                </div>
                <div class="card p-3 flex flex-col gap-2">
                    <span class="text-sm text-muted">"Target address · color (for Log page)"</span>
                    {if addresses.is_empty() {
                        view! { <span class="text-xs text-dim">"—"</span> }.into_view()
                    } else {
                        view! {
                            <div class="flex flex-col gap-2 mt-1">
                                {addresses.into_iter().map(|addr| {
                                    let addr_for_color = addr.clone();
                                    let color = move || {
                                        target_colors.get().get(&addr_for_color).cloned().unwrap_or_else(|| "#8af".to_string())
                                    };
                                    let addr_clone = addr.clone();
                                    view! {
                                        <div class="flex items-center gap-2 flex-wrap">
                                            <input
                                                type="color"
                                                class="w-8 h-8 cursor-pointer rounded border bg-transparent"
                                                prop:value=color
                                                on:input=move |ev| {
                                                    let val = event_target_value(&ev);
                                                    target_colors.update(|m| { m.insert(addr_clone.clone(), val.clone()); });
                                                    save_target_colors_to_storage(&target_colors.get());
                                                }
                                            />
                                            <span class="text-xs font-mono break-all">{addr}</span>
                                        </div>
                                    }
                                }).collect_view()}
                            </div>
                        }.into_view()
                    }}
                </div>
                <div class="card p-3 flex items-center justify-between gap-2">
                    <span class="text-sm text-muted">"Wallet"</span>
                    <span class="text-xs font-mono break-all max-w-[200px] truncate" title=wallet.clone()>
                        {wallet}
                    </span>
                </div>
                <div class="card p-3 flex items-center justify-between gap-2">
                    <span class="text-sm text-muted">"Delta highlight (sec)"</span>
                    <span class="text-xs tabular-nums">{ui.delta_highlight_sec}</span>
                </div>
                <div class="card p-3 flex items-center justify-between gap-2">
                    <span class="text-sm text-muted">"Delta animation (sec)"</span>
                    <span class="text-xs tabular-nums">{ui.delta_animation_sec}</span>
                </div>
            </div>
        </div>
    }
}

#[component]
fn TopTradersPage() -> impl IntoView {
    let (category, set_category) = create_signal::<String>("OVERALL".to_string());
    let (time_period, set_time_period) = create_signal::<String>("WEEK".to_string());
    let (order_by, set_order_by) = create_signal::<String>("PNL".to_string());
    let leaderboard = create_resource(
        move || (category.get(), time_period.get(), order_by.get()),
        |(c, t, o)| async move { fetch_leaderboard(&c, &t, &o).await },
    );
    view! {
        <div class="flex-1 overflow-auto flex flex-col min-h-0 p-4">
            <h1 class="page-title mb-3">"Top traders"</h1>
            <p class="text-sm text-muted mb-3">"Polymarket leaderboard (by PnL or volume)."</p>
            <div class="flex flex-wrap gap-2 mb-3">
                <label class="text-xs text-muted flex items-center gap-1">
                    "Category"
                    <select
                        class="rounded px-2 py-1 text-xs"
                        on:change=move |ev| {
                            let v = event_target_value(&ev);
                            set_category.set(v);
                        }
                        prop:value=move || category.get()
                    >
                        <option value="OVERALL">"Overall"</option>
                        <option value="POLITICS">"Politics"</option>
                        <option value="SPORTS">"Sports"</option>
                        <option value="CRYPTO">"Crypto"</option>
                        <option value="ECONOMICS">"Economics"</option>
                        <option value="TECH">"Tech"</option>
                        <option value="FINANCE">"Finance"</option>
                        <option value="CULTURE">"Culture"</option>
                    </select>
                </label>
                <label class="text-xs text-muted flex items-center gap-1">
                    "Period"
                    <select
                        class="rounded px-2 py-1 text-xs"
                        on:change=move |ev| {
                            let v = event_target_value(&ev);
                            set_time_period.set(v);
                        }
                        prop:value=move || time_period.get()
                    >
                        <option value="DAY">"Day"</option>
                        <option value="WEEK">"Week"</option>
                        <option value="MONTH">"Month"</option>
                        <option value="ALL">"All"</option>
                    </select>
                </label>
                <label class="text-xs text-muted flex items-center gap-1">
                    "Sort"
                    <select
                        class="rounded px-2 py-1 text-xs"
                        on:change=move |ev| {
                            let v = event_target_value(&ev);
                            set_order_by.set(v);
                        }
                        prop:value=move || order_by.get()
                    >
                        <option value="PNL">"PnL"</option>
                        <option value="VOL">"Volume"</option>
                    </select>
                </label>
            </div>
            {move || {
                match leaderboard.get() {
                    None => view! {
                        <p class="text-muted text-sm">"Loading…"</p>
                    }.into_view(),
                    Some(Err(e)) => view! {
                        <p class="text-red-400 text-sm">"Error: " {e}</p>
                    }.into_view(),
                    Some(Ok(entries)) => {
                        if entries.is_empty() {
                            view! { <p class="text-muted text-sm">"No entries."</p> }.into_view()
                        } else {
                            view! {
                                <div class="overflow-x-auto border rounded-lg">
                                    <table class="w-full border-collapse text-xs">
                                        <thead>
                                            <tr class="bg-surface">
                                                <th class="p-2 text-left text-muted font-medium border-b border">"#"</th>
                                                <th class="p-2 text-left text-muted font-medium border-b border">"Address"</th>
                                                <th class="p-2 text-right text-muted font-medium border-b border">"Volume"</th>
                                                <th class="p-2 text-right text-muted font-medium border-b border">"PnL"</th>
                                                <th class="p-2 text-left text-muted font-medium border-b border">"X"</th>
                                            </tr>
                                        </thead>
                                        <tbody>
                                            {entries
                                                .into_iter()
                                                .enumerate()
                                                .map(|(i, e)| {
                                                    let rank = e.rank.clone().unwrap_or_else(|| (i + 1).to_string());
                                                    let addr = e.proxy_wallet.clone().unwrap_or_else(|| "—".to_string());
                                                    let vol = e.vol.map(|v| format!("{:.0}", v)).unwrap_or_else(|| "—".to_string());
                                                    let pnl = e.pnl.map(|p| {
                                                        let s = format!("{:.2}", p);
                                                        if p >= 0.0 { format!("+{}", s) } else { s }
                                                    }).unwrap_or_else(|| "—".to_string());
                                                    let pnl_class = e.pnl.map(|p| if p >= 0.0 { "side-buy" } else { "side-sell" }).unwrap_or_else(|| "text-muted");
                                                    let x_user = e.x_username.clone().unwrap_or_else(|| "—".to_string());
                                                    view! {
                                                        <tr class="border-b border hover:bg-surface-elevated">
                                                            <td class="p-2 text-muted tabular-nums">{rank}</td>
                                                            <td class="p-2 font-mono text-[11px]">
                                                                <span class="break-all" title=addr.clone()>{addr}</span>
                                                            </td>
                                                            <td class="p-2 text-right text-muted tabular-nums">{vol}</td>
                                                            <td class=format!("p-2 text-right tabular-nums {}", pnl_class)>{pnl}</td>
                                                            <td class="p-2 text-muted">{x_user}</td>
                                                        </tr>
                                                    }
                                                })
                                                .collect_view()}
                                        </tbody>
                                    </table>
                                </div>
                            }.into_view()
                        }
                    }
                }
            }}
        </div>
    }
}

#[component]
fn PositionsPanel(
    target_addresses: Vec<String>,
    positions: std::collections::HashMap<String, Vec<PositionSummary>>,
    delta_highlight_sec: u64,
    _delta_animation_sec: u64,
) -> impl IntoView {
    let users = if target_addresses.is_empty() {
        positions.keys().cloned().collect::<Vec<_>>()
    } else {
        target_addresses
    };
    view! {
        <div class="card p-3 flex-1 min-h-0 flex flex-col overflow-hidden">
            <h3 class="section-label mb-2">"Live positions"</h3>
            {if users.is_empty() {
                view! { <p class="text-muted text-xs">"No targets"</p> }.into_view()
            } else {
                view! {
                    <div class="overflow-y-auto overflow-x-hidden flex-1 min-h-0">
                        {users
                            .into_iter()
                            .map(|addr| {
                                let pos_key = positions
                                    .keys()
                                    .find(|k| k.to_lowercase() == addr.to_lowercase())
                                    .cloned();
                                let pos = pos_key
                                    .and_then(|k| positions.get(&k).cloned())
                                    .unwrap_or_default();
                                view! {
                                    <div class="mb-4 last:mb-0">
                                        <span class="font-mono text-[11px] break-all" style="color: var(--accent)">{addr.clone()}</span>
                                        <div class="mt-1.5 mb-2 text-[11px] p-2 bg-surface-elevated border rounded">
                                            {pos.len()} " position(s)"
                                        </div>
                                        <table class="w-full text-[11px] border-collapse">
                                            <thead>
                                                <tr>
                                                    <th class="text-muted font-medium text-left py-1 pr-2 border-b border">"Slug"</th>
                                                    <th class="text-muted font-medium text-left py-1 pr-2 border-b border">"Outcome"</th>
                                                    <th class="text-muted font-medium text-left py-1 pr-2 border-b border">"Size"</th>
                                                    <th class="text-muted font-medium text-left py-1 pr-2 border-b border">"Price"</th>
                                                    <th class="text-muted font-medium text-right py-1 pr-0 border-b border">"Δ"</th>
                                                </tr>
                                            </thead>
                                            <tbody>
                                                {pos
                                                    .into_iter()
                                                    .map(|p| {
                                                        let delta_class = match p.delta {
                                                            Some(d) if d > 0.0 => "side-buy font-semibold",
                                                            Some(_) => "side-sell font-semibold",
                                                            None => "",
                                                        };
                                                        let delta_str = p
                                                            .delta
                                                            .map(|d| if d > 0.0 { format!("+{:.2}", d) } else { format!("{:.2}", d) })
                                                            .unwrap_or_default();
                                                        view! {
                                                            <tr class="border-b border-[#2a2a2a] last:border-0">
                                                                <td class="py-1 pr-2 break-words" title=p.slug.clone()>
                                                                    {p.slug}
                                                                </td>
                                                                <td class="py-1 pr-2 font-medium">{p.outcome}</td>
                                                                <td class="py-1 pr-2 text-muted tabular-nums whitespace-nowrap">
                                                                    {p.size}
                                                                </td>
                                                                <td class="py-1 pr-2 text-muted tabular-nums whitespace-nowrap">
                                                                    {p.cur_price}
                                                                </td>
                                                                <td class=format!("py-1 text-right tabular-nums min-w-[3.5em] {}", delta_class)>
                                                                    {delta_str}
                                                                </td>
                                                            </tr>
                                                        }
                                                    })
                                                    .collect_view()}
                                            </tbody>
                                        </table>
                                    </div>
                                }
                            })
                            .collect_view()}
                    </div>
                }
                    .into_view()
            }}
        </div>
    }
}

/// Value for "show all targets" in the log filter.
const LOG_TARGET_ALL: &str = "";

#[component]
pub fn App() -> impl IntoView {
    view! {
        <Router>
            <AppInner/>
        </Router>
    }
}

#[component]
fn AppInner() -> impl IntoView {
    let (state, set_state) = create_signal::<Option<BotState>>(None);
    let (selected_log_target, set_selected_log_target) = create_signal::<Option<String>>(None);
    let target_colors = create_rw_signal::<std::collections::HashMap<String, String>>(load_target_colors_from_storage());
    let theme = create_rw_signal::<String>(load_theme());
    let sidebar_open = create_rw_signal(false);
    let location = use_location();
    let path = move || {
        let p = location.pathname.get();
        if p.is_empty() { "/".to_string() } else { p }
    };
    create_effect(move |_| {
        let t = theme.get();
        if let Some(window) = web_sys::window() {
            if let Some(doc) = window.document() {
                if let Some(html) = doc.document_element() {
                    let _ = html.set_attribute("data-theme", &t);
                }
            }
        }
    });

    create_effect(move |_| {
        spawn_local(async move {
            if let Ok(s) = fetch_state().await {
                set_state.set(Some(s));
            }
        });
    });

    create_effect(move |_| {
        use std::sync::Once;
        static START: Once = Once::new();
        let set_state = set_state.clone();
        START.call_once(move || {
            let set_state_interval = set_state.clone();
            let _ = gloo_timers::callback::Interval::new(5000, move || {
                spawn_local({
                    let set_state = set_state_interval.clone();
                    async move {
                        if let Ok(s) = fetch_state().await {
                            set_state.set(Some(s));
                        }
                    }
                });
            });
            if let Ok(es) = EventSource::new("/api/state/stream") {
                let set_state_sse = set_state.clone();
                let closure =
                    wasm_bindgen::closure::Closure::<dyn FnMut(web_sys::MessageEvent)>::new(
                        move |_e: web_sys::MessageEvent| {
                            let set_state = set_state_sse.clone();
                            spawn_local(async move {
                                if let Ok(s) = fetch_state().await {
                                    set_state.set(Some(s));
                                }
                            });
                        },
                    );
                es.set_onmessage(Some(closure.as_ref().unchecked_ref()));
                closure.forget();
            }
        });
    });

    let state_slice = move || state.get();
    create_effect(move |_| {
        if let Some(s) = state_slice() {
            if let Some(addrs) = &s.status.target_addresses {
                let mut map = target_colors.get();
                let mut updated = false;
                for a in addrs {
                    if !map.contains_key(a) {
                        map.insert(a.clone(), random_hex_color());
                        updated = true;
                    }
                }
                if updated {
                    target_colors.set(map.clone());
                    save_target_colors_to_storage(&map);
                }
            }
        }
    });
    let mode = move || {
        state_slice()
            .as_ref()
            .map(|s| s.status.mode.clone())
            .unwrap_or_else(|| "—".to_string())
    };
    let targets = move || state_slice().as_ref().map(|s| s.status.targets).unwrap_or(0);
    let target_addresses = move || {
        state_slice()
            .as_ref()
            .and_then(|s| s.status.target_addresses.clone())
            .unwrap_or_default()
    };
    let ui = move || {
        state_slice()
            .as_ref()
            .map(|s| s.ui.clone())
            .unwrap_or_default()
    };
    let is_log_page = move || path() == "/logs";
    let no_aside: Option<()> = None;

    let menu_icon = "<svg xmlns='http://www.w3.org/2000/svg' width='20' height='20' viewBox='0 0 24 24' fill='none' stroke='currentColor' stroke-width='2'><line x1='3' y1='6' x2='21' y2='6'/><line x1='3' y1='12' x2='21' y2='12'/><line x1='3' y1='18' x2='21' y2='18'/></svg>";
    view! {
        <Layout
            nav=view! { <Sidebar theme=theme sidebar_open=sidebar_open/> }
            header=view! {
                <button
                    class="menu-btn"
                    on:click=move |_| sidebar_open.update(|o| *o = !*o)
                >
                    <span inner_html=menu_icon></span>
                </button>
                <span
                    class=move || {
                        if mode() == "Live" {
                            "pill pill-live"
                        } else {
                            "pill pill-sim"
                        }
                    }
                >
                    {move || mode().as_str().to_string()}
                </span>
                {move || {
                    if is_log_page() {
                        let addrs = target_addresses();
                        let current = selected_log_target.get();
                        view! {
                            <div class="card flex flex-col gap-1 p-2">
                                <label for="log-target-select" class="text-muted text-xs">"Log target"</label>
                                <select
                                    id="log-target-select"
                                    class="max-w-[280px] text-xs font-mono"
                                    on:change=move |ev| {
                                        let val = event_target_value(&ev);
                                        set_selected_log_target.set(if val.is_empty() { None } else { Some(val) });
                                    }
                                    prop:value=move || {
                                        current.as_deref().unwrap_or(LOG_TARGET_ALL).to_string()
                                    }
                                >
                                    <option value="">"All targets"</option>
                                    {addrs.into_iter().map(|addr| {
                                        let label = if addr.len() > 14 {
                                            format!("{}…", &addr[..14])
                                        } else {
                                            addr.clone()
                                        };
                                        view! { <option value=addr.clone()>{label}</option> }
                                    }).collect_view()}
                                </select>
                            </div>
                        }.into_view()
                    } else {
                        view! {
                            <div class="text-sm text-muted">
                                <span>{format!("{} target(s)", targets())}</span>
                                {target_addresses()
                                    .into_iter()
                                    .map(|addr| view! { <span class="font-mono text-xs text-dim break-all block">{addr}</span> })
                                    .collect_view()}
                            </div>
                        }.into_view()
                    }
                }}
            }
            main=view! {
                {move || {
                    let p = path();
                    if p == "/logs" {
                        let _ = state_slice();
                        let logs_fn = move || state_slice().as_ref().map(|s| s.logs.clone()).unwrap_or_default();
                        let target_fn = move || selected_log_target.get();
                        let loading_fn = move || state_slice().is_none();
                        view! {
                            <div class="flex-1 min-h-0 flex flex-col overflow-hidden">
                                <LogPage logs=logs_fn selected_target=target_fn target_colors=target_colors loading=loading_fn/>
                            </div>
                        }.into_view()
                    } else if p == "/settings" {
                        view! {
                            <div class="flex-1 min-h-0 flex flex-col overflow-hidden">
                                <SettingsPage state=state_slice() target_colors=target_colors/>
                            </div>
                        }.into_view()
                    } else if p == "/toptraders" {
                        view! {
                            <div class="flex-1 min-h-0 flex flex-col overflow-hidden">
                                <TopTradersPage/>
                            </div>
                        }.into_view()
                    } else if p == "/agent" {
                        view! {
                            <div class="flex-1 min-h-0 flex flex-col overflow-hidden">
                                <AgentPage/>
                            </div>
                        }.into_view()
                    } else if p == "/portfolio" {
                        view! {
                            <div class="flex-1 min-h-0 flex flex-col overflow-hidden">
                                <PortfolioPage state=state_slice()/>
                            </div>
                        }.into_view()
                    } else {
                        view! {
                            <div class="flex-1 min-h-0 flex flex-col overflow-hidden">
                                <DashboardPage state=state_slice()/>
                            </div>
                        }.into_view()
                    }
                }}
            }
            theme=theme
            sidebar_open=sidebar_open
            aside=no_aside
        />
    }
}

#[cfg(feature = "csr")]
#[wasm_bindgen::prelude::wasm_bindgen(start)]
pub fn main() {
    console_error_panic_hook::set_once();
    leptos::mount_to_body(App);
}
