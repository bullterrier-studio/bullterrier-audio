//! The seam between code that prepares input and reads the answer of a model, and what runs the model.

use crate::Error;

/// A block of numbers with a shape: what goes into a model and what comes out. Always `shape` multiplied out is the
/// number of `values`.
#[derive(Debug, Clone, PartialEq)]
pub struct Tensor {
    shape: Vec<usize>,
    values: Vec<f32>,
}

impl Tensor {
    /// A tensor of `values` laid out in row-major order with `shape`.
    ///
    /// # Errors
    /// [`Error::Invalid`] naming the shape and the number of values when they do not match.
    pub fn new(shape: Vec<usize>, values: Vec<f32>) -> Result<Self, Error> {
        let cells = shape.iter().try_fold(1_usize, |cells, &side| cells.checked_mul(side));
        if cells != Some(values.len()) {
            return Err(Error::Invalid(format!(
                "tensor shape {shape:?} does not fit {} values",
                values.len()
            )));
        }
        Ok(Self { shape, values })
    }

    /// The length of each axis.
    #[must_use]
    pub fn shape(&self) -> &[usize] {
        &self.shape
    }

    /// The values in row-major order.
    #[must_use]
    pub fn values(&self) -> &[f32] {
        &self.values
    }
}

/// Something that runs a model: the seam between the code that prepares a sound and reads the answer, and the runtime.
/// A test implements it with a stand-in that returns outputs it has built by hand.
pub trait Infer: Send + Sync {
    /// Runs the model on `input`; returns every output of the model, in the model's output order.
    ///
    /// # Errors
    /// [`Error::Invalid`] when the model cannot run on this input.
    fn infer(&self, input: &Tensor) -> Result<Vec<Tensor>, Error>;
}

/// The graph optimisation the models run under, as the key of the stems on disk names it: the optimised graph can
/// change the numbers, so a change here must change the key. `models::OrtInfer` opens its sessions with the level of
/// this name.
pub(crate) const GRAPH_OPTIMISATION: &str = "disable";
