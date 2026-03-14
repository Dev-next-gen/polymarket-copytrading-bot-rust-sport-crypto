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
) -> impl IntoView {
    view! {
        <div class="flex h-screen max-h-screen overflow-hidden">
            <nav class="w-[140px] shrink-0 border border-[#333] bg-[#1a1a1a] flex flex-col">
                {nav}
            </nav>
            <div class="flex flex-1 min-w-0 flex-col overflow-hidden p-3 gap-2">
                <header class="shrink-0 flex items-center gap-3">{header}</header>
                <div class="flex flex-1 min-h-0 overflow-hidden gap-0">
                    <main class="flex-1 min-w-0 overflow-hidden flex flex-col">{main}</main>
                    {match aside {
                        Some(a) => view! { <aside class="w-[420px] min-w-[320px] shrink-0 overflow-hidden flex flex-col p-3">{a}</aside> }.into_view(),
                        None => view! { <div style="display: none;"></div> }.into_view(),
                    }}
                </div>
            </div>
        </div>
    }
}

#[component]
fn Sidebar() -> impl IntoView {
    let location = use_location();
    let path = move || location.pathname.get();
    view! {
        <div class="flex flex-col py-2">
            <A
                href="/"
                class=move || {
                    let binding = path();
                    let p = binding.trim_end_matches('/');
                    if p.is_empty() || p == "/" { "sidebar-link active" }
                    else { "sidebar-link" }
                }
            >
                "Dashboard"
            </A>
            <A
                href="/logs"
                class=move || {
                    let binding = path();
                    if binding.trim_end_matches('/') == "/logs" { "sidebar-link active" }
                    else { "sidebar-link" }
                }
            >
                "Log"
            </A>
            <A
                href="/settings"
                class=move || {
                    let binding = path();
                    if binding.trim_end_matches('/') == "/settings" { "sidebar-link active" }
                    else { "sidebar-link" }
                }
            >
                "Settings"
            </A>
            <A
                href="/toptraders"
                class=move || {
                    let binding = path();
                    if binding.trim_end_matches('/') == "/toptraders" { "sidebar-link active" }
                    else { "sidebar-link" }
                }
            >
                "Top Traders"
            </A>
        </div>
    }
}

#[component]
fn LogPage(
    logs: impl Fn() -> Vec<TradeLog> + 'static,
    selected_target: impl Fn() -> Option<String> + 'static,
    target_colors: RwSignal<std::collections::HashMap<String, String>>,
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
            view! {
                <div class="flex-1 overflow-auto overflow-x-auto min-h-0 flex flex-col">
                    <p class="text-[11px] text-[#666] mb-2 shrink-0">
                        "Showing " {rows.len()} " buy/sell activities (newest first)."
                    </p>
                    <table class="w-full border-collapse text-xs">
                        <thead>
                            <tr>
                                <th class="p-2 text-left text-[#888] font-medium border-b border-[#333]">"Time"</th>
                                <th class="p-2 text-left text-[#888] font-medium border-b border-[#333]">"Tag"</th>
                                <th class="p-2 text-left text-[#888] font-medium border-b border-[#333]">"Side"</th>
                                <th class="p-2 text-left text-[#888] font-medium border-b border-[#333]">"Outcome"</th>
                                <th class="p-2 text-left text-[#888] font-medium border-b border-[#333]">"Size"</th>
                                <th class="p-2 text-left text-[#888] font-medium border-b border-[#333]">"Price"</th>
                                <th class="p-2 text-left text-[#888] font-medium border-b border-[#333]">"Market"</th>
                                <th class="p-2 text-left text-[#888] font-medium border-b border-[#333]">"Target"</th>
                                <th class="p-2 text-left text-[#888] font-medium border-b border-[#333]">"Status"</th>
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
                            let r_slug = r.slug.clone();
                            let r_target = r.target_address.clone();
                            let r_status = r.copy_status.clone();
                            let colors = target_colors.get();
                            let cell_color = r_target.as_ref().and_then(|a| colors.get(a).cloned()).unwrap_or_else(|| "#8af".to_string());
                            view! {
                                <tr key=format!("{:?}-{}", r_time, i) class="border-b border-[#333]">
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
                                    <td class="p-2">{r_price}</td>
                                    <td class="p-2 text-[#ccc] break-words">{r_slug}</td>
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
                </div>
            }
        }}
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
        <div class="flex-1 overflow-auto text-[#888] p-4">
            <h1 class="text-lg font-medium text-[#ccc] mb-2">"Dashboard"</h1>
            <p class="text-sm mb-4">"Overview and current status."</p>
            <div class="grid gap-3 max-w-md">
                <div class="rounded-lg border border-[#333] bg-[#1a1a1a] p-3">
                    <span class="text-xs text-[#666]">"Mode"</span>
                    <p class="text-[#aaa]">{mode.clone()}</p>
                </div>
                <div class="rounded-lg border border-[#333] bg-[#1a1a1a] p-3">
                    <span class="text-xs text-[#666]">"Targets"</span>
                    <p class="text-[#aaa]">{targets} " target(s)"</p>
                    {if !addresses.is_empty() {
                        view! {
                            <div class="text-[#666] text-xs mt-1 font-mono break-all space-y-0.5">
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
                <div class="rounded-lg border border-[#333] bg-[#1a1a1a] p-3">
                    <span class="text-xs text-[#666]">"Recent activity"</span>
                    {if recent.is_empty() {
                        view! { <p class="text-[#666] text-sm">"No activity yet."</p> }.into_view()
                    } else {
                        view! {
                            <ul class="text-[#aaa] text-sm mt-1 space-y-1">
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
        <div class="flex-1 overflow-auto text-[#888] p-4">
            <h1 class="text-lg font-medium text-[#ccc] mb-2">"Settings"</h1>
            <p class="text-sm mb-4">"Current bot configuration. Target colors apply to the Log page."</p>
            <div class="flex flex-col gap-3 max-w-md">
                <div class="rounded-lg border border-[#333] bg-[#1a1a1a] p-3 flex items-center justify-between gap-2">
                    <span class="text-sm text-[#aaa]">"Mode"</span>
                    <span class="text-xs text-[#ccc] font-medium">{mode}</span>
                </div>
                <div class="rounded-lg border border-[#333] bg-[#1a1a1a] p-3 flex items-center justify-between gap-2">
                    <span class="text-sm text-[#aaa]">"Targets"</span>
                    <span class="text-xs text-[#ccc] tabular-nums">{targets}</span>
                </div>
                <div class="rounded-lg border border-[#333] bg-[#1a1a1a] p-3 flex flex-col gap-2">
                    <span class="text-sm text-[#aaa]">"Target address · color (for Log page)"</span>
                    {if addresses.is_empty() {
                        view! { <span class="text-xs text-[#666]">"—"</span> }.into_view()
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
                                                class="w-8 h-8 cursor-pointer rounded border border-[#444] bg-transparent"
                                                prop:value=color
                                                on:input=move |ev| {
                                                    let val = event_target_value(&ev);
                                                    target_colors.update(|m| { m.insert(addr_clone.clone(), val.clone()); });
                                                    save_target_colors_to_storage(&target_colors.get());
                                                }
                                            />
                                            <span class="text-xs text-[#ccc] font-mono break-all">{addr}</span>
                                        </div>
                                    }
                                }).collect_view()}
                            </div>
                        }.into_view()
                    }}
                </div>
                <div class="rounded-lg border border-[#333] bg-[#1a1a1a] p-3 flex items-center justify-between gap-2">
                    <span class="text-sm text-[#aaa]">"Wallet"</span>
                    <span class="text-xs text-[#ccc] font-mono break-all max-w-[200px] truncate" title=wallet.clone()>
                        {wallet}
                    </span>
                </div>
                <div class="rounded-lg border border-[#333] bg-[#1a1a1a] p-3 flex items-center justify-between gap-2">
                    <span class="text-sm text-[#aaa]">"Delta highlight (sec)"</span>
                    <span class="text-xs text-[#ccc] tabular-nums">{ui.delta_highlight_sec}</span>
                </div>
                <div class="rounded-lg border border-[#333] bg-[#1a1a1a] p-3 flex items-center justify-between gap-2">
                    <span class="text-sm text-[#aaa]">"Delta animation (sec)"</span>
                    <span class="text-xs text-[#ccc] tabular-nums">{ui.delta_animation_sec}</span>
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
            <h1 class="text-lg font-medium text-[#ccc] mb-3">"Top traders"</h1>
            <p class="text-xs text-[#666] mb-3">"Polymarket leaderboard (by PnL or volume)."</p>
            <div class="flex flex-wrap gap-2 mb-3">
                <label class="text-xs text-[#888] flex items-center gap-1">
                    "Category"
                    <select
                        class="bg-[#1a1a1a] border border-[#444] text-[#ccc] rounded px-2 py-1 text-xs"
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
                <label class="text-xs text-[#888] flex items-center gap-1">
                    "Period"
                    <select
                        class="bg-[#1a1a1a] border border-[#444] text-[#ccc] rounded px-2 py-1 text-xs"
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
                <label class="text-xs text-[#888] flex items-center gap-1">
                    "Sort"
                    <select
                        class="bg-[#1a1a1a] border border-[#444] text-[#ccc] rounded px-2 py-1 text-xs"
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
                        <p class="text-[#888] text-sm">"Loading…"</p>
                    }.into_view(),
                    Some(Err(e)) => view! {
                        <p class="text-red-400 text-sm">"Error: " {e}</p>
                    }.into_view(),
                    Some(Ok(entries)) => {
                        if entries.is_empty() {
                            view! { <p class="text-[#666] text-sm">"No entries."</p> }.into_view()
                        } else {
                            view! {
                                <div class="overflow-x-auto border border-[#333] rounded-lg">
                                    <table class="w-full border-collapse text-xs">
                                        <thead>
                                            <tr class="bg-[#1a1a1a]">
                                                <th class="p-2 text-left text-[#888] font-medium border-b border-[#333]">"#"</th>
                                                <th class="p-2 text-left text-[#888] font-medium border-b border-[#333]">"Address"</th>
                                                <th class="p-2 text-right text-[#888] font-medium border-b border-[#333]">"Volume"</th>
                                                <th class="p-2 text-right text-[#888] font-medium border-b border-[#333]">"PnL"</th>
                                                <th class="p-2 text-left text-[#888] font-medium border-b border-[#333]">"X"</th>
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
                                                    let pnl_class = e.pnl.map(|p| if p >= 0.0 { "text-green-400" } else { "text-red-400" }).unwrap_or_else(|| "text-[#aaa]");
                                                    let x_user = e.x_username.clone().unwrap_or_else(|| "—".to_string());
                                                    view! {
                                                        <tr class="border-b border-[#333] hover:bg-[#252525]">
                                                            <td class="p-2 text-[#aaa] tabular-nums">{rank}</td>
                                                            <td class="p-2 text-[#ccc] font-mono text-[11px]">
                                                                <span class="break-all" title=addr.clone()>{addr}</span>
                                                            </td>
                                                            <td class="p-2 text-right text-[#aaa] tabular-nums">{vol}</td>
                                                            <td class=format!("p-2 text-right tabular-nums {}", pnl_class)>{pnl}</td>
                                                            <td class="p-2 text-[#888]">{x_user}</td>
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
        <div class="rounded-lg border border-[#333] bg-[#1a1a1a] p-3 flex-1 min-h-0 flex flex-col overflow-hidden">
            <h3 class="text-[11px] text-[#888] uppercase mb-2">"Live positions"</h3>
            {if users.is_empty() {
                view! { <p class="text-[#666] text-xs">"No targets"</p> }.into_view()
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
                                        <span class="font-mono text-[11px] text-[#8af] break-all">{addr.clone()}</span>
                                        <div class="mt-1.5 mb-2 text-[11px] p-2 bg-[#252525] border border-[#333] rounded">
                                            {pos.len()} " position(s)"
                                        </div>
                                        <table class="w-full text-[11px] border-collapse">
                                            <thead>
                                                <tr>
                                                    <th class="text-[#888] font-medium text-left py-1 pr-2 border-b border-[#333]">"Slug"</th>
                                                    <th class="text-[#888] font-medium text-left py-1 pr-2 border-b border-[#333]">"Outcome"</th>
                                                    <th class="text-[#888] font-medium text-left py-1 pr-2 border-b border-[#333]">"Size"</th>
                                                    <th class="text-[#888] font-medium text-left py-1 pr-2 border-b border-[#333]">"Price"</th>
                                                    <th class="text-[#888] font-medium text-right py-1 pr-0 border-b border-[#333]">"Δ"</th>
                                                </tr>
                                            </thead>
                                            <tbody>
                                                {pos
                                                    .into_iter()
                                                    .map(|p| {
                                                        let delta_class = match p.delta {
                                                            Some(d) if d > 0.0 => "text-[#6f6] font-semibold",
                                                            Some(_) => "text-[#f66] font-semibold",
                                                            None => "",
                                                        };
                                                        let delta_str = p
                                                            .delta
                                                            .map(|d| if d > 0.0 { format!("+{:.2}", d) } else { format!("{:.2}", d) })
                                                            .unwrap_or_default();
                                                        view! {
                                                            <tr class="border-b border-[#2a2a2a] last:border-0">
                                                                <td class="py-1 pr-2 text-[#ccc] break-words" title=p.slug.clone()>
                                                                    {p.slug}
                                                                </td>
                                                                <td class="py-1 pr-2 text-[#aaa] font-medium">{p.outcome}</td>
                                                                <td class="py-1 pr-2 text-[#888] tabular-nums whitespace-nowrap">
                                                                    {p.size}
                                                                </td>
                                                                <td class="py-1 pr-2 text-[#888] tabular-nums whitespace-nowrap">
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
    let location = use_location();
    let path = move || {
        let p = location.pathname.get();
        if p.is_empty() { "/".to_string() } else { p }
    };

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

    view! {
        <Layout
            nav=view! { <Sidebar/> }
            header=view! {
                <span
                    class=move || {
                        if mode() == "Live" {
                            "rounded px-2 py-1 text-xs bg-[#1a3d1a] bg-[#2a2a2a]"
                        } else {
                            "rounded px-2 py-1 text-xs bg-[#3d3d1a] bg-[#2a2a2a]"
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
                            <div class="rounded bg-[#2a2a2a] px-2 py-1 text-xs flex flex-col gap-1">
                                <label for="log-target-select" class="text-[#888]">"Log target"</label>
                                <select
                                    id="log-target-select"
                                    class="bg-[#1a1a1a] border border-[#444] text-[#ccc] rounded px-2 py-1 font-mono text-[10px] max-w-[280px]"
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
                            <div class="rounded bg-[#2a2a2a] px-2 py-1 text-xs flex flex-col gap-0.5">
                                <span>{format!("{} target(s)", targets())}</span>
                                {target_addresses()
                                    .into_iter()
                                    .map(|addr| view! { <span class="font-mono text-[10px] text-[#888] break-all block">{addr}</span> })
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
                        let logs_fn = move || state_slice().as_ref().map(|s| s.logs.clone()).unwrap_or_default();
                        let target_fn = move || selected_log_target.get();
                        view! {
                            <div class="flex-1 min-h-0 flex flex-col overflow-hidden">
                                <LogPage logs=logs_fn selected_target=target_fn target_colors=target_colors/>
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
                    } else {
                        view! {
                            <div class="flex-1 min-h-0 flex flex-col overflow-hidden">
                                <DashboardPage state=state_slice()/>
                            </div>
                        }.into_view()
                    }
                }}
            }
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
