// Include the `openmetrics_data_model` module, which is generated from `proto/openmetrics_data_model.proto`.
pub mod openmetrics_data_model {
    include!(concat!(env!("OUT_DIR"), "/openmetrics.rs"));
}

use crate::metrics::counter::Counter;
use crate::metrics::family::{Family, MetricConstructor};
use crate::metrics::{MetricType, TypedMetric};
use crate::registry::{Registry, Unit};
use std::ops::Deref;

pub fn encode<M>(registry: &Registry<M>) -> openmetrics_data_model::MetricSet
where
    M: EncodeMetric,
{
    // MetricSet
    let mut metric_set = openmetrics_data_model::MetricSet::default();

    for (desc, metric) in registry.iter() {
        // MetricFamily
        let mut family = openmetrics_data_model::MetricFamily::default();
        // MetricFamily.name
        family.name = desc.name().to_string();
        // MetricFamily.type
        family.r#type = {
            let metric_type: openmetrics_data_model::MetricType = metric.metric_type().into();
            metric_type as i32
        };
        // MetricFamily.unit
        if let Some(unit) = desc.unit() {
            family.unit = match unit {
                Unit::Amperes => "amperes",
                Unit::Bytes => "bytes",
                Unit::Celsius => "celsius",
                Unit::Grams => "grams",
                Unit::Joules => "joules",
                Unit::Meters => "meters",
                Unit::Ratios => "ratios",
                Unit::Seconds => "seconds",
                Unit::Volts => "volts",
                Unit::Other(other) => other.as_str(),
            }
            .to_string();
        }
        // MetricFamily.help
        family.help = desc.help().to_string();
        println!("family.help: {}", family.help);
        // MetricFamily.Metric
        family.metrics = metric.encode(desc.labels().encode());
        metric_set.metric_families.push(family);
    }

    metric_set
}

impl From<MetricType> for openmetrics_data_model::MetricType {
    fn from(m: MetricType) -> Self {
        match m {
            MetricType::Counter => openmetrics_data_model::MetricType::Counter,
            MetricType::Gauge => openmetrics_data_model::MetricType::Gauge,
            MetricType::Histogram => openmetrics_data_model::MetricType::Histogram,
            MetricType::Info => openmetrics_data_model::MetricType::Info,
            MetricType::Unknown => openmetrics_data_model::MetricType::Unknown,
        }
    }
}

/// Trait implemented by each metric type, e.g. [`Counter`], to implement its encoding.
pub trait EncodeMetric {
    fn encode(
        &self,
        labels: Vec<openmetrics_data_model::Label>,
    ) -> Vec<openmetrics_data_model::Metric>;

    fn metric_type(&self) -> MetricType;
}

impl EncodeMetric for Box<dyn EncodeMetric> {
    fn encode(
        &self,
        labels: Vec<openmetrics_data_model::Label>,
    ) -> Vec<openmetrics_data_model::Metric> {
        self.deref().encode(labels)
    }

    fn metric_type(&self) -> MetricType {
        self.deref().metric_type()
    }
}

pub trait SendEncodeMetric: EncodeMetric + Send {}

impl<T: EncodeMetric + Send> SendEncodeMetric for T {}

impl EncodeMetric for Box<dyn SendEncodeMetric> {
    fn encode(
        &self,
        labels: Vec<openmetrics_data_model::Label>,
    ) -> Vec<openmetrics_data_model::Metric> {
        self.deref().encode(labels)
    }

    fn metric_type(&self) -> MetricType {
        self.deref().metric_type()
    }
}

pub trait EncodeLabel {
    fn encode(&self) -> Vec<openmetrics_data_model::Label>;
}

impl<K: ToString, V: ToString> EncodeLabel for (K, V) {
    fn encode(&self) -> Vec<openmetrics_data_model::Label> {
        let mut label = openmetrics_data_model::Label::default();
        label.name = self.0.to_string();
        label.value = self.1.to_string();
        vec![label]
    }
}

impl<T: EncodeLabel> EncodeLabel for Vec<T> {
    fn encode(&self) -> Vec<openmetrics_data_model::Label> {
        let mut label = vec![];
        for t in self {
            label.append(&mut t.encode());
        }
        label
    }
}

impl<T: EncodeLabel> EncodeLabel for &[T] {
    fn encode(&self) -> Vec<openmetrics_data_model::Label> {
        let mut label = vec![];
        for t in self.iter() {
            label.append(&mut t.encode());
        }
        label
    }
}

/////////////////////////////////////////////////////////////////////////////////
// Counter

impl EncodeMetric for Counter {
    fn encode(
        &self,
        labels: Vec<openmetrics_data_model::Label>,
    ) -> Vec<openmetrics_data_model::Metric> {
        let mut metric = openmetrics_data_model::Metric::default();
        metric.labels = labels;

        metric.metric_points = {
            let mut metric_point = openmetrics_data_model::MetricPoint::default();
            metric_point.value = {
                let mut counter_value = openmetrics_data_model::CounterValue::default();
                counter_value.total = Some(openmetrics_data_model::counter_value::Total::IntValue(
                    self.get(),
                ));
                Some(openmetrics_data_model::metric_point::Value::CounterValue(
                    counter_value,
                ))
            };

            vec![metric_point]
        };

        vec![metric]
    }

    fn metric_type(&self) -> MetricType {
        MetricType::Counter
    }
}

/////////////////////////////////////////////////////////////////////////////////
// Family

impl<S, M, C> EncodeMetric for Family<S, M, C>
where
    S: Clone + std::hash::Hash + Eq + EncodeLabel,
    M: EncodeMetric + TypedMetric,
    C: MetricConstructor<M>,
{
    fn encode(
        &self,
        labels: Vec<openmetrics_data_model::Label>,
    ) -> Vec<openmetrics_data_model::Metric> {
        let mut metrics = vec![];

        let guard = self.read();
        for (label_set, metric) in guard.iter() {
            let mut label = label_set.encode();
            label.append(&mut labels.clone());
            metrics.extend(metric.encode(label));
        }

        metrics
    }

    fn metric_type(&self) -> MetricType {
        M::TYPE
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metrics::counter::Counter;
    use crate::metrics::family::Family;
    use crate::registry::Unit;
    use std::borrow::Cow;

    #[test]
    fn test_encode() {
        let mut registry: Registry<Box<dyn EncodeMetric>> = Registry::default();

        let counter: Counter = Counter::default();
        registry.register_with_unit(
            "my_counter",
            "My counter",
            Unit::Seconds,
            Box::new(counter.clone()),
        );
        counter.inc();

        let family = Family::<Vec<(String, String)>, Counter>::default();
        let sub_registry =
            registry.sub_registry_with_label((Cow::Borrowed("my_key"), Cow::Borrowed("my_value")));
        sub_registry.register(
            "my_counter_family",
            "My counter family",
            Box::new(family.clone()),
        );
        family
            .get_or_create(&vec![
                ("method".to_string(), "GET".to_string()),
                ("status".to_string(), "200".to_string()),
            ])
            .inc();
        family
            .get_or_create(&vec![
                ("method".to_string(), "POST".to_string()),
                ("status".to_string(), "503".to_string()),
            ])
            .inc();

        println!("{:?}", encode(&registry));
    }
}
