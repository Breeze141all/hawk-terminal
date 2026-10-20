use super::theme::ThemeColor;
use super::{
    BaseSeriesSource, IndicatorColorSpec, IndicatorConfig, IndicatorHistoryRequirement,
    IndicatorId, IndicatorOutputRef,
};
use crate::chart::Basis;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::{BTreeMap, BTreeSet};

pub const MIN_LINE_WIDTH: u8 = 1;
pub const MAX_LINE_WIDTH: u8 = 4;

const DEFAULT_INDICATOR_INSTANCE_ID_BASE: u64 = 1 << 63;
const MAX_INDICATOR_HISTORY_BARS: u64 = 50_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct IndicatorInstanceId(pub u64);

impl IndicatorInstanceId {
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    pub fn default_for(kind: &IndicatorId) -> Self {
        Self(DEFAULT_INDICATOR_INSTANCE_ID_BASE + kind.default_instance_slot())
    }
}

impl Serialize for IndicatorInstanceId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_u64(self.0)
    }
}

impl<'de> Deserialize<'de> for IndicatorInstanceId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        u64::deserialize(deserializer).map(Self::new)
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LineStrokeStyle {
    #[default]
    Solid,
    Dashed,
    Dotted,
}

impl LineStrokeStyle {
    pub const ALL: [Self; 3] = [Self::Solid, Self::Dashed, Self::Dotted];
}

impl std::fmt::Display for LineStrokeStyle {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Solid => "Solid",
            Self::Dashed => "Dashed",
            Self::Dotted => "Dotted",
        })
    }
}

/// Definition-local style overrides keyed by stable declaration IDs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct IndicatorStyleOverrides {
    #[serde(default = "default_fill_visible")]
    pub fill_visible: bool,
    #[serde(default)]
    colors: BTreeMap<String, ThemeColor>,
    #[serde(default)]
    line_widths: BTreeMap<String, u8>,
    #[serde(default)]
    stroke_styles: BTreeMap<String, LineStrokeStyle>,
}

fn default_fill_visible() -> bool {
    true
}

impl Default for IndicatorStyleOverrides {
    fn default() -> Self {
        Self {
            fill_visible: true,
            colors: BTreeMap::new(),
            line_widths: BTreeMap::new(),
            stroke_styles: BTreeMap::new(),
        }
    }
}

impl IndicatorStyleOverrides {
    pub fn color(&self, id: &str) -> Option<ThemeColor> {
        self.colors.get(id).copied()
    }

    pub fn set_color(&mut self, id: impl Into<String>, color: ThemeColor) -> bool {
        let id = id.into();
        if id.trim().is_empty() {
            return false;
        }
        if self.color(&id) == Some(color) {
            return false;
        }
        self.colors.insert(id, color);
        true
    }

    pub fn line_width(&self, id: &str) -> Option<u8> {
        self.line_widths.get(id).copied()
    }

    pub fn stroke_style(&self, id: &str) -> Option<LineStrokeStyle> {
        self.stroke_styles.get(id).copied()
    }

    pub fn set_line_width(&mut self, id: impl Into<String>, width: u8) -> bool {
        let id = id.into();
        if id.trim().is_empty() {
            return false;
        }
        let width = width.clamp(MIN_LINE_WIDTH, MAX_LINE_WIDTH);
        if self.line_width(&id) == Some(width) {
            return false;
        }
        self.line_widths.insert(id, width);
        true
    }

    pub fn set_stroke_style(&mut self, id: impl Into<String>, style: LineStrokeStyle) -> bool {
        let id = id.into();
        if id.trim().is_empty() {
            return false;
        }
        if self.stroke_style(&id) == Some(style) {
            return false;
        }
        self.stroke_styles.insert(id, style);
        true
    }

    pub fn normalized(mut self) -> Self {
        for width in self.line_widths.values_mut() {
            *width = (*width).clamp(MIN_LINE_WIDTH, MAX_LINE_WIDTH);
        }
        self.line_widths.retain(|id, _| !id.trim().is_empty());
        self.stroke_styles.retain(|id, _| !id.trim().is_empty());
        self.colors.retain(|id, _| !id.trim().is_empty());
        self
    }

    pub fn normalized_for_slots(self, slots: &[IndicatorColorSpec]) -> Self {
        let mut normalized = self.normalized();
        normalized
            .colors
            .retain(|id, _| slots.iter().any(|slot| slot.id == *id));
        normalized
            .line_widths
            .retain(|id, _| slots.iter().any(|slot| slot.id == *id));
        normalized
            .stroke_styles
            .retain(|id, _| slots.iter().any(|slot| slot.id == *id));
        normalized
    }

    pub fn clear(&mut self) -> bool {
        let changed = !self.fill_visible
            || !self.colors.is_empty()
            || !self.line_widths.is_empty()
            || !self.stroke_styles.is_empty();
        self.colors.clear();
        self.line_widths.clear();
        self.stroke_styles.clear();
        self.fill_visible = true;
        changed
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IndicatorInstance {
    pub id: IndicatorInstanceId,
    #[serde(rename = "indicator")]
    pub kind: IndicatorId,
    pub config: IndicatorConfig,
    pub style_overrides: IndicatorStyleOverrides,
    #[serde(default)]
    pub source_bindings: BTreeMap<String, IndicatorOutputRef>,
    #[serde(default)]
    pub base_source_selections: BTreeMap<String, BaseSeriesSource>,
}

impl IndicatorInstance {
    pub fn new(id: IndicatorInstanceId, kind: IndicatorId) -> Self {
        let config = IndicatorConfig::default_for(&kind);
        Self {
            id,
            kind,
            config,
            style_overrides: IndicatorStyleOverrides::default(),
            source_bindings: BTreeMap::new(),
            base_source_selections: BTreeMap::new(),
        }
    }

    pub fn normalized_for_basis(mut self, basis: Basis) -> Self {
        self.config = self.config.clone().normalized_for_basis(&self.kind, basis);
        self.style_overrides = if self.kind.is_registered() {
            self.style_overrides
                .normalized_for_slots(&self.kind.definition().style_slots)
        } else {
            self.style_overrides.normalized()
        };
        for input_id in self.source_bindings.keys().cloned().collect::<Vec<_>>() {
            self.base_source_selections.remove(&input_id);
        }
        self
    }
}

#[derive(Debug, Clone, Default)]
pub struct IndicatorInstanceStore {
    instances: Vec<IndicatorInstance>,
    next_id: u64,
    warmup_bars_by_id: BTreeMap<IndicatorInstanceId, Option<u64>>,
}

impl PartialEq for IndicatorInstanceStore {
    fn eq(&self, other: &Self) -> bool {
        self.instances == other.instances && self.next_id == other.next_id
    }
}

impl IndicatorInstanceStore {
    pub fn from_instances(instances: Vec<IndicatorInstance>, basis: Basis) -> Self {
        let mut store = Self::default();
        for instance in instances {
            if !instance.kind.is_registered() {
                continue;
            }
            let instance = instance.normalized_for_basis(basis);
            if !instance.config.is_valid_for(&instance.kind) {
                continue;
            }
            if store.get(instance.id).is_none() {
                store.next_id = store.next_id.max(
                    instance
                        .id
                        .0
                        .checked_add(1)
                        .filter(|next| *next < DEFAULT_INDICATOR_INSTANCE_ID_BASE)
                        .unwrap_or(1),
                );
                store.instances.push(instance);
            }
        }
        store.rebuild_warmup_cache();
        store.next_id = store.next_id.max(1);
        store
    }

    pub fn instances(&self) -> &[IndicatorInstance] {
        &self.instances
    }

    pub fn get(&self, id: IndicatorInstanceId) -> Option<&IndicatorInstance> {
        self.instances.iter().find(|instance| instance.id == id)
    }

    pub(crate) fn warmup_bars(&self, id: IndicatorInstanceId) -> Option<u64> {
        self.warmup_bars_by_id.get(&id).copied().unwrap_or_else(|| {
            self.get(id)
                .and_then(|instance| instance.kind.warmup_bars(&instance.config))
        })
    }

    pub fn create(&mut self, kind: IndicatorId) -> IndicatorInstanceId {
        while self
            .instances
            .iter()
            .any(|instance| instance.id == IndicatorInstanceId::new(self.next_id))
        {
            self.next_id += 1;
        }
        let id = IndicatorInstanceId::new(self.next_id);
        self.next_id += 1;
        let instance = IndicatorInstance::new(id, kind);
        self.warmup_bars_by_id
            .insert(id, instance.kind.warmup_bars(&instance.config));
        self.instances.push(instance);
        id
    }

    pub fn update_config(&mut self, id: IndicatorInstanceId, config: IndicatorConfig) -> bool {
        let Some(instance) = self.instances.iter_mut().find(|instance| instance.id == id) else {
            return false;
        };
        let config = config.normalized_for(&instance.kind);
        if !config.is_valid_for(&instance.kind) {
            return false;
        }
        if instance.config == config {
            return false;
        }
        instance.config = config;
        let warmup_bars = instance.kind.warmup_bars(&instance.config);
        self.warmup_bars_by_id.insert(id, warmup_bars);
        true
    }

    pub fn reset_config(&mut self, id: IndicatorInstanceId) -> bool {
        let Some(kind) = self.get(id).map(|instance| instance.kind.clone()) else {
            return false;
        };
        self.update_config(id, IndicatorConfig::default_for(&kind))
    }

    pub fn update_source_binding(
        &mut self,
        id: IndicatorInstanceId,
        input_id: impl Into<String>,
        output: Option<IndicatorOutputRef>,
    ) -> bool {
        let Some(instance) = self.instances.iter_mut().find(|instance| instance.id == id) else {
            return false;
        };
        let input_id = input_id.into();
        if !instance
            .kind
            .definition()
            .source_inputs
            .iter()
            .any(|input| input.id == input_id)
        {
            return false;
        }
        match output {
            Some(output) => {
                let binding_changed = instance.source_bindings.get(&input_id) != Some(&output);
                let base_selection_removed = instance.base_source_selections.remove(&input_id);
                instance.source_bindings.insert(input_id, output);
                binding_changed || base_selection_removed.is_some()
            }
            None => instance.source_bindings.remove(&input_id).is_some(),
        }
    }

    pub fn update_base_source(
        &mut self,
        id: IndicatorInstanceId,
        input_id: impl Into<String>,
        source: Option<BaseSeriesSource>,
    ) -> bool {
        let Some(instance) = self.instances.iter_mut().find(|instance| instance.id == id) else {
            return false;
        };
        let input_id = input_id.into();
        if !instance
            .kind
            .definition()
            .source_inputs
            .iter()
            .any(|input| input.id == input_id)
        {
            return false;
        }
        if instance.source_bindings.contains_key(&input_id) {
            return false;
        }
        match source {
            Some(source) => {
                if instance.base_source_selections.get(&input_id) == Some(&source) {
                    return false;
                }
                instance.base_source_selections.insert(input_id, source);
                true
            }
            None => instance.base_source_selections.remove(&input_id).is_some(),
        }
    }

    pub fn update_color(
        &mut self,
        id: IndicatorInstanceId,
        slot: impl Into<String>,
        color: ThemeColor,
    ) -> bool {
        self.instances
            .iter_mut()
            .find(|instance| instance.id == id)
            .is_some_and(|instance| instance.style_overrides.set_color(slot, color))
    }

    pub fn update_fill_visibility(&mut self, id: IndicatorInstanceId, visible: bool) -> bool {
        self.instances
            .iter_mut()
            .find(|instance| instance.id == id)
            .is_some_and(|instance| {
                if instance.style_overrides.fill_visible == visible {
                    return false;
                }
                instance.style_overrides.fill_visible = visible;
                true
            })
    }

    pub fn update_line_width(
        &mut self,
        id: IndicatorInstanceId,
        slot: impl Into<String>,
        width: u8,
    ) -> bool {
        self.instances
            .iter_mut()
            .find(|instance| instance.id == id)
            .is_some_and(|instance| instance.style_overrides.set_line_width(slot, width))
    }

    pub fn update_stroke_style(
        &mut self,
        id: IndicatorInstanceId,
        slot: impl Into<String>,
        stroke_style: LineStrokeStyle,
    ) -> bool {
        self.instances
            .iter_mut()
            .find(|instance| instance.id == id)
            .is_some_and(|instance| {
                instance
                    .style_overrides
                    .set_stroke_style(slot, stroke_style)
            })
    }

    pub fn reset_style(&mut self, id: IndicatorInstanceId) -> bool {
        let Some(instance) = self.instances.iter_mut().find(|instance| instance.id == id) else {
            return false;
        };
        instance.style_overrides.clear()
    }

    pub fn remove(&mut self, id: IndicatorInstanceId) -> Option<IndicatorInstance> {
        let index = self
            .instances
            .iter()
            .position(|instance| instance.id == id)?;
        self.warmup_bars_by_id.remove(&id);
        Some(self.instances.remove(index))
    }

    pub fn history_requirement(
        &self,
        active: &[IndicatorInstanceId],
        basis: Basis,
        reference_time: exchange::UnixMs,
    ) -> IndicatorHistoryRequirement {
        let active_set = active
            .iter()
            .copied()
            .filter(|id| self.get(*id).is_some())
            .collect::<BTreeSet<_>>();
        let mut memo = BTreeMap::new();
        active
            .iter()
            .filter_map(|id| {
                history_requirement_for_instance(
                    *id,
                    self,
                    &active_set,
                    basis,
                    reference_time,
                    &mut memo,
                    &mut BTreeSet::new(),
                )
            })
            .fold(Default::default(), IndicatorHistoryRequirement::merge)
    }

    pub fn dependency_warmup_bars(&self, active: &[IndicatorInstanceId]) -> u64 {
        let active_set = active
            .iter()
            .copied()
            .filter(|id| self.get(*id).is_some())
            .collect::<BTreeSet<_>>();
        let mut memo = BTreeMap::new();
        active
            .iter()
            .filter_map(|id| {
                dependency_warmup_for_instance(
                    *id,
                    self,
                    &active_set,
                    &mut memo,
                    &mut BTreeSet::new(),
                )
            })
            .max()
            .unwrap_or(0)
    }

    pub fn normalized_for_basis(&self, basis: Basis) -> Self {
        let mut normalized = Self {
            instances: self
                .instances
                .iter()
                .cloned()
                .map(|instance| instance.normalized_for_basis(basis))
                .collect(),
            next_id: self.next_id,
            warmup_bars_by_id: BTreeMap::new(),
        };
        normalized.rebuild_warmup_cache();
        normalized
    }

    pub fn restore_snapshot(&mut self, snapshot: Self) {
        self.instances = snapshot.instances;
        self.next_id = self.next_id.max(snapshot.next_id);
        self.rebuild_warmup_cache();
    }

    fn rebuild_warmup_cache(&mut self) {
        self.warmup_bars_by_id = self
            .instances
            .iter()
            .map(|instance| (instance.id, instance.kind.warmup_bars(&instance.config)))
            .collect();
    }
}

/// Why a declared source binding cannot act as an indicator dependency.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceBindingProblem {
    UnknownInput(String),
    InputDisallowsIndicator(String),
    SelfReference,
    MissingInstance(IndicatorInstanceId),
    MissingOutput(String),
    NotSourceableOutput(String),
}

impl std::fmt::Display for SourceBindingProblem {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownInput(input) => {
                write!(
                    formatter,
                    "source binding references unknown input '{input}'"
                )
            }
            Self::InputDisallowsIndicator(input) => write!(
                formatter,
                "source input '{input}' does not allow indicator bindings"
            ),
            Self::SelfReference => formatter.write_str("indicator cannot depend on itself"),
            Self::MissingInstance(id) => write!(
                formatter,
                "source binding references missing indicator instance {id:?}"
            ),
            Self::MissingOutput(channel) => write!(
                formatter,
                "source binding references missing output '{channel}'"
            ),
            Self::NotSourceableOutput(channel) => write!(
                formatter,
                "source binding output '{channel}' is not a sourceable numeric plot output"
            ),
        }
    }
}

impl std::error::Error for SourceBindingProblem {}

impl IndicatorInstanceStore {
    /// Resolves `source_id` on `instance` to the instance it draws from.
    ///
    /// `Ok(None)` means the binding is absent, so the input contributes nothing.
    /// `Err` means the binding is present but cannot act as a dependency.
    pub fn dependency_target<'a>(
        &'a self,
        instance: &IndicatorInstance,
        source_id: &str,
        active: &BTreeSet<IndicatorInstanceId>,
    ) -> Result<Option<&'a IndicatorInstance>, SourceBindingProblem> {
        let definition = instance.kind.definition();
        let Some(input) = definition
            .source_inputs
            .iter()
            .find(|input| input.id == source_id)
        else {
            return Err(SourceBindingProblem::UnknownInput(source_id.to_owned()));
        };
        if !input.allow_indicator {
            return Err(SourceBindingProblem::InputDisallowsIndicator(
                source_id.to_owned(),
            ));
        }
        let Some(reference) = instance.source_bindings.get(source_id) else {
            return Ok(None);
        };
        if reference.instance_id == instance.id {
            return Err(SourceBindingProblem::SelfReference);
        }
        if !active.contains(&reference.instance_id) {
            return Err(SourceBindingProblem::MissingInstance(reference.instance_id));
        }
        let Some(target) = self.get(reference.instance_id) else {
            return Err(SourceBindingProblem::MissingInstance(reference.instance_id));
        };
        let target_definition = target.kind.definition();
        if target_definition.output(&reference.channel_id).is_none() {
            return Err(SourceBindingProblem::MissingOutput(
                reference.channel_id.clone(),
            ));
        }
        if target_definition
            .sourceable_numeric_output(&reference.channel_id)
            .is_none()
        {
            return Err(SourceBindingProblem::NotSourceableOutput(
                reference.channel_id.clone(),
            ));
        }
        Ok(Some(target))
    }

    /// Dependency edges among `active` instances, optionally ignoring one binding.
    pub fn dependency_edges(
        &self,
        active: &BTreeSet<IndicatorInstanceId>,
        excluded: Option<(IndicatorInstanceId, &str)>,
    ) -> BTreeMap<IndicatorInstanceId, Vec<IndicatorInstanceId>> {
        let mut edges = BTreeMap::<IndicatorInstanceId, Vec<IndicatorInstanceId>>::new();
        for id in active {
            let Some(instance) = self.get(*id) else {
                continue;
            };
            let mut targets = Vec::new();
            for source_id in instance.source_bindings.keys() {
                if excluded == Some((*id, source_id.as_str())) {
                    continue;
                }
                if let Ok(Some(target)) = self.dependency_target(instance, source_id, active)
                    && !targets.contains(&target.id)
                {
                    targets.push(target.id);
                }
            }
            if !targets.is_empty() {
                edges.insert(*id, targets);
            }
        }
        edges
    }
}

fn dependency_warmup_for_instance(
    id: IndicatorInstanceId,
    store: &IndicatorInstanceStore,
    active: &BTreeSet<IndicatorInstanceId>,
    memo: &mut BTreeMap<IndicatorInstanceId, u64>,
    visiting: &mut BTreeSet<IndicatorInstanceId>,
) -> Option<u64> {
    if let Some(warmup) = memo.get(&id) {
        return Some(*warmup);
    }
    if !visiting.insert(id) {
        return Some(0);
    }
    let instance = store.get(id)?;
    let own = store.warmup_bars(id).unwrap_or_default();
    let dependency = instance
        .source_bindings
        .keys()
        .filter_map(|source_id| {
            store
                .dependency_target(instance, source_id, active)
                .ok()
                .flatten()
                .and_then(|target| {
                    dependency_warmup_for_instance(target.id, store, active, memo, visiting)
                })
        })
        .max()
        .unwrap_or(0);
    visiting.remove(&id);
    let warmup = own
        .saturating_add(dependency)
        .min(MAX_INDICATOR_HISTORY_BARS);
    memo.insert(id, warmup);
    Some(warmup)
}

fn history_requirement_for_instance(
    id: IndicatorInstanceId,
    store: &IndicatorInstanceStore,
    active: &BTreeSet<IndicatorInstanceId>,
    basis: Basis,
    reference_time: exchange::UnixMs,
    memo: &mut BTreeMap<IndicatorInstanceId, IndicatorHistoryRequirement>,
    visiting: &mut BTreeSet<IndicatorInstanceId>,
) -> Option<IndicatorHistoryRequirement> {
    if let Some(requirement) = memo.get(&id) {
        return Some(*requirement);
    }
    if !visiting.insert(id) {
        return Some(IndicatorHistoryRequirement::default());
    }
    let instance = store.get(id)?;
    let own = instance.kind.history_requirement_with_warmup(
        instance.config.clone(),
        basis,
        reference_time,
        store.warmup_bars(id),
    );
    let dependency = instance
        .source_bindings
        .keys()
        .filter_map(|source_id| {
            store
                .dependency_target(instance, source_id, active)
                .ok()
                .flatten()
                .and_then(|target| {
                    history_requirement_for_instance(
                        target.id,
                        store,
                        active,
                        basis,
                        reference_time,
                        memo,
                        visiting,
                    )
                })
        })
        .fold(Default::default(), IndicatorHistoryRequirement::merge);
    visiting.remove(&id);
    let history_start = match (own.history_start(), dependency.history_start()) {
        (Some(left), Some(right)) => Some(left.min(right)),
        (Some(time), None) | (None, Some(time)) => Some(time),
        (None, None) => None,
    };
    let requirement = IndicatorHistoryRequirement::new(
        history_start,
        own.history_bars()
            .saturating_add(dependency.history_bars())
            .min(MAX_INDICATOR_HISTORY_BARS),
    );
    memo.insert(id, requirement);
    Some(requirement)
}
