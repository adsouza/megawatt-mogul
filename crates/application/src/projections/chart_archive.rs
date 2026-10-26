//! Pure storage decomposition. The canonical projection state retains its source schema; joining monthly
//! chart blobs reconstructs precisely the same bytes before boundary verification.
use super::{CompletedDay, Projections, TimelinePoint, TimelineProjection};
use domain::{
    demand::ForecastPoint,
    state::{State, StateError},
    time::SimMonth,
};
use std::collections::{BTreeMap, VecDeque};
#[derive(Debug, Clone, PartialEq)]
pub struct ArchivedChartMonth {
    pub month: SimMonth,
    pub bytes: Vec<u8>,
}
struct LiveChart {
    points: VecDeque<TimelinePoint>,
    forecast: Option<[ForecastPoint; 24]>,
    island_forecasts: BTreeMap<domain::ids::NodeId, [ForecastPoint; 24]>,
}
struct ChartMonth {
    month: SimMonth,
    completed: Option<CompletedDay>,
    live: Option<LiveChart>,
}
domain::state_record!(LiveChart { points, forecast; since4 { island_forecasts: Default::default() } });
domain::state_record!(ChartMonth {
    month,
    completed,
    live
});
impl Projections {
    pub fn split_charts(
        self,
        current: SimMonth,
    ) -> Result<(Self, Vec<ArchivedChartMonth>), StateError> {
        self.split_charts_schema(current, domain::state::CURRENT_STATE_SCHEMA)
    }
    pub fn split_charts_schema(
        mut self,
        current: SimMonth,
        schema: u32,
    ) -> Result<(Self, Vec<ArchivedChartMonth>), StateError> {
        let mut months: BTreeMap<SimMonth, ChartMonth> = BTreeMap::new();
        let timeline = std::mem::take(&mut self.timeline);
        for completed in timeline.completed {
            let month = completed.month;
            if months
                .insert(
                    month,
                    ChartMonth {
                        month,
                        completed: Some(completed),
                        live: None,
                    },
                )
                .is_some()
            {
                return Err(StateError::Invalid);
            }
        }
        if !timeline.points.is_empty() || timeline.forecast.is_some() {
            let month = if timeline.forecast.is_some() {
                current
            } else {
                months.last_key_value().map_or(current, |(m, _)| *m)
            };
            months
                .entry(month)
                .or_insert(ChartMonth {
                    month,
                    completed: None,
                    live: None,
                })
                .live = Some(LiveChart {
                points: timeline.points,
                forecast: timeline.forecast,
                island_forecasts: timeline.island_forecasts,
            });
        }
        let charts = months
            .into_iter()
            .map(|(month, data)| ArchivedChartMonth {
                month,
                bytes: data.state_bytes_schema(schema),
            })
            .collect();
        Ok((self, charts))
    }
    pub fn restore_charts(self, charts: Vec<ArchivedChartMonth>) -> Result<Self, StateError> {
        self.restore_charts_schema(charts, domain::state::CURRENT_STATE_SCHEMA)
    }
    pub fn restore_charts_schema(
        mut self,
        charts: Vec<ArchivedChartMonth>,
        schema: u32,
    ) -> Result<Self, StateError> {
        if self.timeline != TimelineProjection::default() {
            return Err(StateError::Invalid);
        }
        let mut previous = None;
        let mut live = false;
        for chart in charts {
            if previous.is_some_and(|m| m >= chart.month) {
                return Err(StateError::Invalid);
            }
            previous = Some(chart.month);
            let data = ChartMonth::from_state_bytes_schema(&chart.bytes, schema)?;
            if data.month != chart.month || data.completed.is_none() && data.live.is_none() {
                return Err(StateError::Invalid);
            }
            if let Some(completed) = data.completed {
                if completed.month != chart.month {
                    return Err(StateError::Invalid);
                }
                self.timeline.completed.push(completed);
            }
            if let Some(value) = data.live {
                if live {
                    return Err(StateError::Invalid);
                }
                live = true;
                self.timeline.points = value.points;
                self.timeline.forecast = value.forecast;
                self.timeline.island_forecasts = value.island_forecasts;
            }
        }
        Ok(self)
    }
}
