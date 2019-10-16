use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use std::sync::Arc;

#[derive(Default, Debug)]
pub struct PbProperties {
    pub set_style: Option<ProgressStyle>,
    pub enable_steady_tick: Option<u64>,
    pub set_length: Option<u64>,
    pub set_message: Option<String>,
    pub set_prefix: Option<String>,
    pub reset: bool,
    pub inc: Option<u64>,
}

#[derive(Default, Debug)]
pub struct Indicator {
    pub pb_total: Option<ProgressBar>,
    pub pb_item: Option<ProgressBar>,
    active_pb: u8, // 0 means pb_total is active, 1 means another.
}

impl Indicator {
    pub fn new(multi_bar: Option<Arc<MultiProgress>>) -> Self {
        if let Some(mb) = multi_bar.as_ref() {
            let pb_total = ProgressBar::new(!0);
            let ps = ProgressStyle::default_spinner(); // {spinner} {msg}
            pb_total.set_style(ps);
            let pb_total = mb.add(pb_total);

            let pb_item = ProgressBar::new(!0);
            let ps = ProgressStyle::default_spinner(); // {spinner} {msg}
            pb_item.set_style(ps);
            let pb_item = mb.add(pb_item);

            Indicator {
                pb_total: Some(pb_total),
                pb_item: Some(pb_item),
                active_pb: 0,
            }
        } else {
            Self::default()
        }
    }

    pub fn is_some(&self) -> bool {
        self.pb_total.is_some()
    }

    pub fn active_pb_total(&mut self) -> &Self {
        self.active_pb = 0;
        self
    }

    pub fn active_pb_item(&mut self) -> &Self {
        self.active_pb = 1;
        self
    }

    fn get_active_pb(&self) -> Option<&ProgressBar> {
        if self.active_pb == 0 {
            self.pb_total.as_ref()
        } else {
            self.pb_item.as_ref()
        }
    }

    #[allow(dead_code)]
    pub fn set_length(&self, len: u64) {
        if let Some(pb) = self.get_active_pb() {
            pb.set_length(len);
        }
    }

    pub fn alter_pb(&self, pb_properties: PbProperties) {
        if let Some(pb) = self.get_active_pb() {
            if let Some(style) = pb_properties.set_style {
                pb.set_style(style);
            }

            if let Some(length) = pb_properties.set_length {
                pb.set_length(length);
            }

            if let Some(tick) = pb_properties.enable_steady_tick {
                pb.enable_steady_tick(tick);
            }

            if let Some(message) = pb_properties.set_message {
                pb.set_message(message.as_str());
            }

            if pb_properties.reset {
                pb.reset();
            }

            if let Some(prefix) = pb_properties.set_prefix {
                pb.set_prefix(prefix.as_str());
            }

            if let Some(inc) = pb_properties.inc {
                pb.inc(inc);
            }
        }
    }

    pub fn inc_pb_total(&self, inc: u64) {
        if let Some(pb) = self.pb_total.as_ref() {
            pb.inc(inc);
        }
    }

    pub fn inc_pb_item(&self, inc: u64) {
        if let Some(pb) = self.pb_item.as_ref() {
            pb.inc(inc);
        }
    }

    pub fn inc_pb(&self, inc: u64) {
        if self.active_pb == 0 {
            self.inc_pb_total(inc);
        } else {
            self.inc_pb_item(inc);
        }
    }

    pub fn pb_finish(&self) {
        if let Some(pb) = self.pb_total.as_ref() {
            pb.finish();
        }

        if let Some(pb) = self.pb_item.as_ref() {
            pb.finish_and_clear();
        }
        
    }

    pub fn set_message(&self, message: String) {
        if let Some(pb) = self.get_active_pb() {
            pb.set_message(message.as_str());
        }
    }

    pub fn set_message_pb_total(&self, message: String) {
        if let Some(pb) = self.pb_total.as_ref() {
            pb.set_message(message.as_str());
        }
    }
}
