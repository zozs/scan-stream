use wasm_bindgen::{closure::Closure, JsCast};
use web_sys::{EventSource, EventSourceInit, MessageEvent};
use yew::format::{FormatError, Text};
use yew::prelude::*;
use yew::services::Task;

pub struct EventSourceTask {
    event_source: EventSource,
    _cb: Closure<dyn FnMut(MessageEvent) -> ()>,
}

pub struct EventSourceService {}

impl EventSourceService {
    pub fn new() -> Self {
        EventSourceService {}
    }

    pub fn connect<OUT: 'static>(self, url: &str, callback: Callback<(OUT, OUT)>) -> EventSourceTask
    where
        OUT: From<Text>,
    {
        // let event_source = EventSource::new(url).unwrap();
        // The below is a very convoluted way of doing new EventSource({withCredentials: true}) in Js.
        let mut event_source_init = EventSourceInit::new();
        event_source_init.with_credentials(true);

        let event_source = EventSource::new_with_event_source_init_dict(url, &event_source_init).unwrap();
        let cb = Closure::wrap(Box::new(move |event: MessageEvent| {
            let text = event.data().as_string();
            let data = if let Some(text) = text {
                Ok(text)
            } else {
                Err(FormatError::CantEncodeBinaryAsText.into())
            };
            let out = OUT::from(data);

            // also grab message id and pass it along.
            let message_id = OUT::from(Ok(event.last_event_id()));
            callback.emit((out, message_id));
        }) as Box<dyn FnMut(MessageEvent)>);
        event_source.set_onmessage(Some(cb.as_ref().unchecked_ref()));
        EventSourceTask { event_source, _cb: cb }
    }
}

impl Task for EventSourceTask {
    fn is_active(&self) -> bool {
        self.event_source.ready_state() == EventSource::OPEN
    }
}

impl Drop for EventSourceTask {
    fn drop(&mut self) {
        self.event_source.close();
    }
}
