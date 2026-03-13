//! Leptos UI for Polymarket copy-trading bot. Fetches /api/state and displays logs, dashboard, settings, positions.

use leptos::*;
use serde::Deserialize;

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

#[derive(Clone, Debug, Default, Deserialize)]
pub struct BotState {
    pub logs: Vec<TradeLog>,
    pub status: Status,
    pub positions: std::collections::HashMap<String, Vec<PositionSummary>>,
    pub ui: UiConfig,
}

async fn fetch_state() -> Result<BotState, String> {
    gloo_net::http::Request::get("/api/state")
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())
}

#[component]
fn Layout(
    nav: Children,
    header: Children,
    main: Children,
    #[prop(optional)] aside: Option<Children>,
) -> impl IntoView {
    view! {
        <div class="flex h-screen max-h-screen overflow-hidden">
            <nav class="w-[140px] shrink-0 border border-[#333] bg-[#1a1a1a] flex flex-col">
                {nav()}
            </nav>
            <div class="flex flex-1 min-w-0 flex-col overflow-hidden p-3 gap-2">
                <header class="shrink-0 flex items-center gap-3">{header()}</header>
                <div class="flex flex-1 min-h-0 overflow-hidden gap-0">
                    <main class="flex-1 min-w-0 overflow-hidden flex flex-col">{main()}</main>
                    {match aside {
                        Some(a) => view! { <aside class="w-[420px] min-w-[320px] shrink-0 overflow-hidden flex flex-col p-3">{a()}</aside> }.into_any(),
                        None => view! {}.into_any(),
                    }}
                </div>
            </div>
        </div>
    }
}

type Page = &'static str;
const PAGES: &[(Page, &str)] = &[("dashboard", "Dashboard"), ("log", "Log"), ("setting", "Settings")];

#[component]
fn Sidebar(current: Page, on_navigate: Callback<Page>) -> impl IntoView {
    view! {
        <div class="flex flex-col py-2">
            {PAGES
                .iter()
                .map(|(id, label)| {
                    let is_current = *id == current;
                    let id = *id;
                    view! {
                        <button
                            type="button"
                            on:click=move |_| on_navigate.call(id)
                            class=if is_current {
                                "px-3 py-2 text-left text-sm border-l-[3px] border-[#8af] bg-[#222] text-[#8af]"
                            } else {
                                "px-3 py-2 text-left text-sm border-l-[3px] border-transparent text-[#888] hover:text-[#ccc]"
                            }
                        >
                            {*label}
                        </button>
                    }
                })
                .collect_view()}
        </div>
    }
}

#[component]
fn LogPage(logs: Vec<TradeLog>) -> impl IntoView {
    let rows: Vec<_> = logs.into_iter().rev().collect();
    view! {
        <div class="flex-1 overflow-auto overflow-x-auto min-h-0 flex flex-col">
            <p class="text-[11px] text-[#666] mb-2 shrink-0">
                "Showing " {rows.len()} " log entries (newest first)."
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
                            let time_short = if r.time.len() >= 19 { &r.time[11..19] } else { &r.time[..] };
                            let side_class = if r.side == "BUY" { "text-[#6f6]" } else { "text-[#f66]" };
                            view! {
                                <tr key=format!("{:?}-{}", r.time, i) class="border-b border-[#333]">
                                    <td class="p-2">{time_short}</td>
                                    <td class="p-2">{r.tag}</td>
                                    <td class=format!("p-2 font-medium {}", side_class)>{r.side}</td>
                                    <td class="p-2">{r.outcome}</td>
                                    <td class="p-2 tabular-nums">
                                        {if let Ok(n) = r.size.parse::<f64>() {
                                            format!("{:.2}", n)
                                        } else {
                                            r.size
                                        }}
                                    </td>
                                    <td class="p-2">{r.price}</td>
                                    <td class="p-2 text-[#ccc] break-words">{r.slug}</td>
                                    <td class="p-2 font-mono text-[11px] text-[#8af] break-all">
                                        {r.target_address.unwrap_or_default()}
                                    </td>
                                    <td class="p-2">{r.copy_status.unwrap_or_default()}</td>
                                </tr>
                            }
                        })
                        .collect_view()}
                </tbody>
            </table>
        </div>
    }
}

#[component]
fn DashboardPage(state: Option<BotState>) -> impl IntoView {
    let mode = state.as_ref().map(|s| s.status.mode.as_str()).unwrap_or("—");
    let targets = state.as_ref().map(|s| s.status.targets).unwrap_or(0);
    let addresses = state.as_ref().and_then(|s| s.status.target_addresses.clone()).unwrap_or_default();
    let recent: Vec<_> = state
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
                    <p class="text-[#aaa]">{mode}</p>
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
                            .into_any()
                    } else {
                        view! {}.into_any()
                    }}
                </div>
                <div class="rounded-lg border border-[#333] bg-[#1a1a1a] p-3">
                    <span class="text-xs text-[#666]">"Recent activity"</span>
                    {if recent.is_empty() {
                        view! { <p class="text-[#666] text-sm">"No activity yet."</p> }.into_any()
                    } else {
                        view! {
                            <ul class="text-[#aaa] text-sm mt-1 space-y-1">
                                {recent
                                    .into_iter()
                                    .map(|r| {
                                        let t = if r.time.len() >= 19 { &r.time[11..19] } else { &r.time[..] };
                                        view! {
                                            <li>
                                                {t} " " {r.side} " " {r.outcome} " @ " {r.price} " — " {r.slug}
                                            </li>
                                        }
                                    })
                                    .collect_view()}
                            </ul>
                        }
                            .into_any()
                    }}
                </div>
            </div>
        </div>
    }
}

#[component]
fn SettingsPage(state: Option<BotState>) -> impl IntoView {
    let mode = state.as_ref().map(|s| s.status.mode.as_str()).unwrap_or("—");
    let targets = state.as_ref().map(|s| s.status.targets).unwrap_or(0);
    let addresses = state
        .as_ref()
        .and_then(|s| s.status.target_addresses.clone())
        .unwrap_or_default();
    let wallet = state
        .as_ref()
        .and_then(|s| s.status.wallet.clone())
        .unwrap_or_else(|| "—".to_string());
    let ui = state.as_ref().map(|s| &s.ui).unwrap_or(&UiConfig::default());
    view! {
        <div class="flex-1 overflow-auto text-[#888] p-4">
            <h1 class="text-lg font-medium text-[#ccc] mb-2">"Settings"</h1>
            <p class="text-sm mb-4">"Current bot configuration (read-only)."</p>
            <div class="flex flex-col gap-3 max-w-md">
                <div class="rounded-lg border border-[#333] bg-[#1a1a1a] p-3 flex items-center justify-between gap-2">
                    <span class="text-sm text-[#aaa]">"Mode"</span>
                    <span class="text-xs text-[#ccc] font-medium">{mode}</span>
                </div>
                <div class="rounded-lg border border-[#333] bg-[#1a1a1a] p-3 flex items-center justify-between gap-2">
                    <span class="text-sm text-[#aaa]">"Targets"</span>
                    <span class="text-xs text-[#ccc] tabular-nums">{targets}</span>
                </div>
                <div class="rounded-lg border border-[#333] bg-[#1a1a1a] p-3 flex flex-col gap-1">
                    <span class="text-sm text-[#aaa]">"Target addresses"</span>
                    <span class="text-xs text-[#ccc] font-mono break-all">
                        {if addresses.is_empty() {
                            "—".to_string()
                        } else {
                            addresses.join(", ")
                        }}
                    </span>
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
                view! { <p class="text-[#666] text-xs">"No targets"</p> }.into_any()
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
                    .into_any()
            }}
        </div>
    }
}

#[component]
pub fn App() -> impl IntoView {
    let (page, set_page) = signal::<Page>("log");
    let (state, set_state) = signal::<Option<BotState>>(None);

    create_effect(move |_| {
        spawn_local(async move {
            match fetch_state().await {
                Ok(s) => set_state.set(Some(s)),
                Err(_) => {}
            }
        });
    });

    create_effect(move |_| {
        let _ = page();
        let interval = gloo_utils::window()
            .set_interval_with_callback_and_timeout_and_arguments(
                move || {
                    spawn_local(async move {
                        if let Ok(s) = fetch_state().await {
                            set_state.set(Some(s));
                        }
                    });
                },
                3000,
                &[],
            );
        move || {
            if let Some(h) = interval.ok() {
                let _ = gloo_utils::window().clear_interval_with_handle(h);
            }
        }
    });

    let state_slice = move || state.get();
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

    view! {
        <Layout
            nav=view! {
                <Sidebar
                    current=page.get()
                    on_navigate=move |p| set_page.set(p)
                />
            }
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
                <div class="rounded bg-[#2a2a2a] px-2 py-1 text-xs flex flex-col gap-0.5">
                    <span>{move || format!("{} target(s)", targets())}</span>
                    {move || {
                        target_addresses()
                            .into_iter()
                            .map(|addr| view! { <span class="font-mono text-[10px] text-[#888] break-all block">{addr}</span> })
                            .collect_view()
                    }}
                </div>
            }
            main=view! {
                <div class=move || format!("flex-1 min-h-0 flex flex-col overflow-hidden {}", if page.get() != "log" { "hidden" } else { "" }) data-page="log">
                    <LogPage logs=move || state_slice().as_ref().map(|s| s.logs.clone()).unwrap_or_default()/>
                </div>
                <div class=move || format!("flex-1 min-h-0 flex flex-col overflow-hidden {}", if page.get() != "dashboard" { "hidden" } else { "" }) data-page="dashboard">
                    <DashboardPage state=state_slice()/>
                </div>
                <div class=move || format!("flex-1 min-h-0 flex flex-col overflow-hidden {}", if page.get() != "setting" { "hidden" } else { "" }) data-page="setting">
                    <SettingsPage state=state_slice()/>
                </div>
            }
            aside=Some(view! {
                {move || {
                    if page.get() == "log" {
                        view! {
                            <PositionsPanel
                                target_addresses=target_addresses()
                                positions=state_slice().as_ref().map(|s| s.positions.clone()).unwrap_or_default()
                                delta_highlight_sec=ui().delta_highlight_sec
                                _delta_animation_sec=ui().delta_animation_sec
                            />
                        }
                            .into_any()
                    } else {
                        view! {}.into_any()
                    }
                }}
            })
        />
    }
}
