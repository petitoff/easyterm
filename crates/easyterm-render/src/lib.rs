use easyterm_core::Cursor;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RendererPreference {
    Auto,
    Gpu,
    Cpu,
}

impl Default for RendererPreference {
    fn default() -> Self {
        Self::Auto
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RendererKind {
    Gpu,
    Cpu,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrameModel {
    pub lines: Vec<String>,
    pub cursor: Cursor,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RenderStats {
    pub cells_drawn: usize,
}

pub trait RenderBackend {
    fn kind(&self) -> RendererKind;
    fn render(&mut self, frame: &FrameModel) -> RenderStats;
}

#[derive(Debug, Default)]
pub struct GpuRenderer;

impl RenderBackend for GpuRenderer {
    fn kind(&self) -> RendererKind {
        RendererKind::Gpu
    }

    fn render(&mut self, frame: &FrameModel) -> RenderStats {
        RenderStats {
            cells_drawn: frame.lines.iter().map(|line| line.chars().count()).sum(),
        }
    }
}

#[derive(Debug, Default)]
pub struct CpuRenderer;

impl RenderBackend for CpuRenderer {
    fn kind(&self) -> RendererKind {
        RendererKind::Cpu
    }

    fn render(&mut self, frame: &FrameModel) -> RenderStats {
        RenderStats {
            cells_drawn: frame.lines.iter().map(|line| line.chars().count()).sum(),
        }
    }
}

pub fn select_backend(
    preference: RendererPreference,
    gpu_available: bool,
) -> Box<dyn RenderBackend> {
    match preference {
        RendererPreference::Gpu if gpu_available => Box::<GpuRenderer>::default(),
        RendererPreference::Gpu => Box::<CpuRenderer>::default(),
        RendererPreference::Cpu => Box::<CpuRenderer>::default(),
        RendererPreference::Auto if gpu_available => Box::<GpuRenderer>::default(),
        RendererPreference::Auto => Box::<CpuRenderer>::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::{select_backend, FrameModel, RendererKind, RendererPreference};
    use easyterm_core::Cursor;

    #[test]
    fn auto_prefers_gpu_when_available() {
        let backend = select_backend(RendererPreference::Auto, true);
        assert_eq!(backend.kind(), RendererKind::Gpu);
    }

    #[test]
    fn falls_back_to_cpu_when_gpu_unavailable() {
        let backend = select_backend(RendererPreference::Gpu, false);
        assert_eq!(backend.kind(), RendererKind::Cpu);
    }

    #[test]
    fn render_counts_cells() {
        let mut backend = select_backend(RendererPreference::Cpu, true);
        let stats = backend.render(&FrameModel {
            lines: vec!["abc".into(), "de".into()],
            cursor: Cursor { row: 0, col: 0 },
        });

        assert_eq!(stats.cells_drawn, 5);
    }
}
