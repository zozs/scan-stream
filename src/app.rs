use serde_derive::{Deserialize, Serialize};
use serde_json;
use std::collections::BTreeMap;
use std::fmt;
use std::time::Duration;
use yew::format::Text;
use yew::prelude::*;
use yew::services::{
    ConsoleService,
    IntervalService,
    interval::IntervalTask,
    Task,
};

use crate::sse::{EventSourceService, EventSourceTask};

const MERCURE_URL: &str = ".well-known/mercure?topic=https%3A%2F%2Fsome.example.com%2Fstream";

pub struct App {
    state: State,
    link: ComponentLink<Self>,
    console: ConsoleService,
    event_source_task: Option<EventSourceTask>,
    _connection_check_task: IntervalTask,
    _interval_task: IntervalTask,
}

pub struct State {
    scans: BTreeMap<i32, Scan>,
    last_event_id: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct Entry {
    description: String,
    completed: bool,
    editing: bool,
}

#[derive(Copy, Clone)]
pub enum ScanState {
    Scanning(/* Instant */f64), // can't use instant in WASM.
    Scanned(Duration),
    Failed(Duration),
}

pub struct Scan {
    scan_id: i32,
    status: ScanState,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")] // Fixes so that scanId in JSON is scan_id in Rust <3
pub struct ScanStatus {
    scan_id: i32,
    status: ScanStatusState,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "lowercase")] // Fixes so that this matches the string json representation.
pub enum ScanStatusState {
    Scanning,
    Scanned,
    Failed,
}

pub enum Msg {
    ConnectionCheck,
    LogError(String),
    ScanEvent(Vec<ScanStatus>, String),
    Timer,
}

impl App {
    fn connect_sse_task(link: &ComponentLink<Self>, last_event_id: &Option<String>) -> Option<EventSourceTask> {
        let event_source = EventSourceService::new();
        let url = match last_event_id {
            Some(id) => format!("{}&Last-Event-ID={}", MERCURE_URL, id),
            None => MERCURE_URL.to_string(),
        };

        Some(event_source.connect(url.as_str(), link.callback(
            |(events_text, last_event_id): (Text, Text)| {
                match (events_text, last_event_id) {
                    (Ok(events_string), Ok(last_event_id)) =>
                        match serde_json::from_str(&events_string) {
                            Ok(events) => Msg::ScanEvent(events, last_event_id),
                            Err(_) => {
                                Msg::LogError("Could not deserialize Json event.".to_string())
                            }
                        }
                    _ => Msg::LogError("Something weird with event text or last message id :(".to_string())
                }
        })))
    }
}

impl Component for App {
    type Message = Msg;
    type Properties = ();

    fn create(_: Self::Properties, link: ComponentLink<Self>) -> Self {
        let scans = BTreeMap::new();
        let state = State {
            scans,
            last_event_id: None,
        };
        let console = ConsoleService::new();

        let event_source_task = App::connect_sse_task(&link, &state.last_event_id);

        // Periodic timer to send timer event every second.
        let mut interval_service = IntervalService::new();
        let interval_task = interval_service.spawn(Duration::new(1, 0),
            link.callback(|_| Msg::Timer));
        let connection_check_task = interval_service.spawn(Duration::new(10, 0),
            link.callback(|_| Msg::ConnectionCheck));

        App {
            state,
            link,
            console,
            event_source_task,
            _connection_check_task: connection_check_task,
            _interval_task: interval_task,
        }
    }

    fn change(&mut self, _props: Self::Properties) -> ShouldRender {
        false
    }

    fn update(&mut self, msg: Self::Message) -> ShouldRender {
        match msg {
            Msg::ConnectionCheck => {
                // Periodically check that connection isn't closed. If it is, reconnect.
                match &self.event_source_task {
                    Some(task) => {
                        if !task.is_active() {
                            self.console.warn("SSE connection lost. Reconnecting!");
                            self.event_source_task = App::connect_sse_task(&self.link, &self.state.last_event_id);
                        }
                    }
                    None => {
                        self.console.warn("SSE connection lost. Reconnecting!");
                        self.event_source_task = App::connect_sse_task(&self.link, &self.state.last_event_id);
                    }
                }
            }
            Msg::LogError(error) => {
                self.console.log(format!("Got error: {}", error).as_str());
            }
            Msg::ScanEvent(scan_statuses, last_event_id) => {
                // Go through events and update internal state.
                let now = performance_now();

                for e in scan_statuses {
                    self.console.log(format!("received event: {}, id {}", e, last_event_id).as_str());

                    let scan = self.state.scans.entry(e.scan_id).or_insert(Scan { scan_id: e.scan_id, status: ScanState::Scanning(now) } );

                    // If we update scan.status depends on its current value and the new value in e.
                    scan.status = match scan.status {
                        ScanState::Scanning(started) => {
                            match e.status {
                                ScanStatusState::Scanning => scan.status, // if duplicate scanning is received, don't change anything.
                                ScanStatusState::Scanned => ScanState::Scanned(perf_to_duration(now - started)), // calculate final duration.
                                ScanStatusState::Failed => ScanState::Failed(perf_to_duration(now - started)),
                            }
                        },
                        _ => {
                            // All other state transitions (scanned -> scanned, scanned -> failed, etc.) are disallowed.
                            self.console.warn(format!("Tried to update current {} with new event {}", scan, e).as_str());
                            scan.status
                        }
                    };

                    // Remember last handled event id, if we need to reconnect.
                    self.state.last_event_id = Some(last_event_id.clone());
                    
                }
            }
            Msg::Timer => { /* No need to actually do anything, we always return true to ShouldRender */ }
        }
        true
    }

    fn view(&self) -> Html {
        html! {
            <div class="container">
                <section class="section">
                    <h1 class="title">{ "scan stream" }</h1>
                </section>
                <section class="section">
                    <table class="table is-hoverable is-fullwidth">
                        <thead>
                            <th>{ "Scan id" }</th>
                            <th>{ "Elapsed time" }</th>
                            <th>{ "Status" }</th>
                        </thead>
                        <tbody>
                            { for self.state.scans.iter().rev().map(|(_, scan)| self.view_scan(scan)) }
                        </tbody>
                    </table>
                </section>
            </div>
        }
    }
}

impl App {
    fn view_scan(&self, scan: &Scan) -> Html {
        fn duration_to_string(duration: Duration) -> String {
            format!("{} seconds", duration.as_secs())
        }

        let now = performance_now();
        let (tag_class, tag_label, duration) = match scan.status {
            ScanState::Scanning(start) => ("tag is-info", "scanning", perf_to_duration(now - start)),
            ScanState::Scanned(duration) => ("tag is-success", "scanned", duration),
            ScanState::Failed(duration) => ("tag is-danger", "failed", duration),
        };

        html! {
            <tr>
                <td>{ scan.scan_id }</td>
                <td>{ duration_to_string(duration) }</td>
                <td><span class=tag_class>{ tag_label }</span></td>
            </tr>
        }
    }
}

impl fmt::Display for Scan {
    fn fmt(&self, f:&mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "({}, {})", self.scan_id, self.status)
    }
}

impl fmt::Display for ScanState {
    fn fmt(&self, f:&mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ScanState::Scanning(_) => write!(f, "scanning"),
            ScanState::Scanned(_)  => write!(f, "scanned"),
            ScanState::Failed(_)   => write!(f, "failed"),
        }
    }
}

impl fmt::Display for ScanStatus {
    fn fmt(&self, f:&mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "({}, {})", self.scan_id, self.status)
    }
}

impl fmt::Display for ScanStatusState {
    fn fmt(&self, f:&mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ScanStatusState::Scanning => write!(f, "scanning"),
            ScanStatusState::Scanned  => write!(f, "scanned"),
            ScanStatusState::Failed   => write!(f, "failed"),
        }
    }
}

fn perf_to_duration(amt: f64) -> Duration {
    let secs = (amt as u64) / 1_000;
    let nanos = ((amt as u32) % 1_000) * 1_000_000;
    Duration::new(secs, nanos)
}

fn performance_now() -> f64 {
    // let now = Instant::now(); // need something else for wasm below.
    let window = web_sys::window().expect("should have a window in this context");
    let performance = window.performance().expect("performance should be available");
    performance.now()
}
