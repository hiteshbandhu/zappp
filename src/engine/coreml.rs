//! Minimal safe-ish wrapper over CoreML: load a compiled `.mlmodelc`, build
//! multi-arrays, run a prediction, read outputs (respecting strides).
//!
//! Compiled models are plain files, so this needs no Xcode — only the CoreML
//! framework that ships with macOS.

use std::path::Path;

use anyhow::{Result, anyhow};
use objc2::{AnyThread as _, rc::Retained, runtime::{AnyObject, ProtocolObject}};
use objc2_core_ml::{
    MLComputeUnits, MLDictionaryFeatureProvider, MLFeatureProvider, MLFeatureValue, MLModel,
    MLModelConfiguration, MLMultiArray, MLMultiArrayDataType,
};
use objc2_foundation::{NSArray, NSDictionary, NSNumber, NSString, NSURL};

#[derive(Clone, Copy, Debug)]
pub enum Units {
    Cpu,
    CpuAndGpu,
    CpuAndNeuralEngine,
    All,
}

impl Units {
    fn raw(self) -> MLComputeUnits {
        match self {
            Units::Cpu => MLComputeUnits::CPUOnly,
            Units::CpuAndGpu => MLComputeUnits::CPUAndGPU,
            Units::CpuAndNeuralEngine => MLComputeUnits::CPUAndNeuralEngine,
            Units::All => MLComputeUnits::All,
        }
    }

    pub fn from_env(name: &str, default: Units) -> Units {
        match std::env::var(name).as_deref() {
            Ok("cpu") => Units::Cpu,
            Ok("gpu") => Units::CpuAndGpu,
            Ok("ane") => Units::CpuAndNeuralEngine,
            Ok("all") => Units::All,
            _ => default,
        }
    }
}

pub struct Model(Retained<MLModel>);

// CoreML models are safe to use from any thread (not concurrently; callers
// serialize access behind a Mutex).
unsafe impl Send for Model {}

impl Model {
    pub fn load(path: &Path, units: Units) -> Result<Self> {
        let url = NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()));
        unsafe {
            let config = MLModelConfiguration::new();
            config.setComputeUnits(units.raw());
            let model = MLModel::modelWithContentsOfURL_configuration_error(&url, &config)
                .map_err(|e| anyhow!("CoreML could not load {}: {}", path.display(), e.localizedDescription()))?;
            Ok(Self(model))
        }
    }

    pub fn predict(&self, inputs: &[(&str, &Array)]) -> Result<Outputs> {
        unsafe {
            let keys: Vec<Retained<NSString>> = inputs.iter().map(|(k, _)| NSString::from_str(k)).collect();
            let values: Vec<Retained<MLFeatureValue>> = inputs
                .iter()
                .map(|(_, a)| MLFeatureValue::featureValueWithMultiArray(&a.0))
                .collect();
            let key_refs: Vec<&NSString> = keys.iter().map(|k| &**k).collect();
            let value_refs: Vec<&AnyObject> = values.iter().map(|v| v.as_ref() as &AnyObject).collect();
            let dict: Retained<NSDictionary<NSString, AnyObject>> = NSDictionary::from_slices(&key_refs, &value_refs);
            let provider = MLDictionaryFeatureProvider::initWithDictionary_error(
                MLDictionaryFeatureProvider::alloc(),
                &dict,
            )
            .map_err(|e| anyhow!("bad CoreML inputs: {}", e.localizedDescription()))?;
            let out = self
                .0
                .predictionFromFeatures_error(ProtocolObject::from_ref(&*provider))
                .map_err(|e| anyhow!("CoreML prediction failed: {}", e.localizedDescription()))?;
            Ok(Outputs(out))
        }
    }
}

/// Compiles a `.mlpackage` (or `.mlmodel`) into a `.mlmodelc` at `dest`.
/// Done once at install; loading a compiled model is much faster.
pub fn compile(package: &Path, dest: &Path) -> Result<()> {
    let url = NSURL::fileURLWithPath(&NSString::from_str(&package.to_string_lossy()));
    #[allow(deprecated)]
    let compiled = unsafe { MLModel::compileModelAtURL_error(&url) }
        .map_err(|e| anyhow!("CoreML could not compile {}: {}", package.display(), e.localizedDescription()))?;
    let temp = compiled
        .path()
        .map(|p| std::path::PathBuf::from(p.to_string()))
        .ok_or_else(|| anyhow!("CoreML returned no compiled model"))?;
    let _ = std::fs::remove_dir_all(dest);
    // The temporary copy may live on another volume; fall back to copying.
    if std::fs::rename(&temp, dest).is_err() {
        let status = std::process::Command::new("ditto").arg(&temp).arg(dest).status()?;
        let _ = std::fs::remove_dir_all(&temp);
        if !status.success() {
            return Err(anyhow!("could not move the compiled model into place"));
        }
    }
    Ok(())
}

pub struct Outputs(Retained<ProtocolObject<dyn MLFeatureProvider>>);

impl Outputs {
    pub fn array(&self, name: &str) -> Result<Array> {
        unsafe {
            let value = self
                .0
                .featureValueForName(&NSString::from_str(name))
                .ok_or_else(|| anyhow!("missing CoreML output {name}"))?;
            let array = value
                .multiArrayValue()
                .ok_or_else(|| anyhow!("CoreML output {name} is not an array"))?;
            Ok(Array(array))
        }
    }
}

pub struct Array(Retained<MLMultiArray>);

unsafe impl Send for Array {}

impl Array {
    fn new(shape: &[usize], ty: MLMultiArrayDataType) -> Result<Self> {
        let dims: Vec<Retained<NSNumber>> = shape.iter().map(|d| NSNumber::new_isize(*d as isize)).collect();
        let shape = NSArray::from_retained_slice(&dims);
        unsafe {
            let array = MLMultiArray::initWithShape_dataType_error(MLMultiArray::alloc(), &shape, ty)
                .map_err(|e| anyhow!("could not allocate MLMultiArray: {}", e.localizedDescription()))?;
            Ok(Self(array))
        }
    }

    /// Zero-filled float32 array (freshly allocated arrays are contiguous).
    pub fn f32_zeros(shape: &[usize]) -> Result<Self> {
        let a = Self::new(shape, MLMultiArrayDataType::Float32)?;
        a.f32_mut().fill(0.);
        Ok(a)
    }

    /// Float16 array filled from `values` (row-major).
    pub fn f16_from(shape: &[usize], values: &[f32]) -> Result<Self> {
        let a = Self::new(shape, MLMultiArrayDataType::Float16)?;
        assert_eq!(a.count(), values.len());
        unsafe {
            #[allow(deprecated)]
            let ptr = a.0.dataPointer().as_ptr() as *mut u16;
            let out = std::slice::from_raw_parts_mut(ptr, values.len());
            for (o, v) in out.iter_mut().zip(values) {
                *o = half::f16::from_f32(*v).to_bits();
            }
        }
        Ok(a)
    }

    pub fn i32_from(shape: &[usize], values: &[i32]) -> Result<Self> {
        let a = Self::new(shape, MLMultiArrayDataType::Int32)?;
        a.i32_mut().copy_from_slice(values);
        Ok(a)
    }

    fn count(&self) -> usize {
        unsafe { self.0.count() as usize }
    }

    pub fn shape(&self) -> Vec<usize> {
        unsafe { self.0.shape().iter().map(|n| n.as_isize() as usize).collect() }
    }

    fn strides(&self) -> Vec<usize> {
        unsafe { self.0.strides().iter().map(|n| n.as_isize() as usize).collect() }
    }

    fn data_type(&self) -> MLMultiArrayDataType {
        unsafe { self.0.dataType() }
    }

    /// Mutable view of a contiguous float32 array we allocated ourselves.
    #[allow(clippy::mut_from_ref)]
    pub fn f32_mut(&self) -> &mut [f32] {
        assert_eq!(self.data_type(), MLMultiArrayDataType::Float32);
        unsafe {
            #[allow(deprecated)]
            let ptr = self.0.dataPointer().as_ptr() as *mut f32;
            std::slice::from_raw_parts_mut(ptr, self.count())
        }
    }

    #[allow(clippy::mut_from_ref)]
    fn i32_mut(&self) -> &mut [i32] {
        assert_eq!(self.data_type(), MLMultiArrayDataType::Int32);
        unsafe {
            #[allow(deprecated)]
            let ptr = self.0.dataPointer().as_ptr() as *mut i32;
            std::slice::from_raw_parts_mut(ptr, self.count())
        }
    }

    /// A read view that resolves strides and the data pointer once, so hot
    /// loops don't cross into Objective-C per element.
    pub fn view(&self) -> View<'_> {
        unsafe {
            #[allow(deprecated)]
            let base = self.0.dataPointer().as_ptr() as *const u8;
            View {
                base,
                strides: self.strides(),
                ty: self.data_type(),
                _array: self,
            }
        }
    }

    pub fn get_i32(&self, index: &[usize]) -> i32 {
        self.view().i32(index)
    }
}

pub struct View<'a> {
    base: *const u8,
    strides: Vec<usize>,
    ty: MLMultiArrayDataType,
    _array: &'a Array,
}

impl View<'_> {
    #[inline]
    fn offset(&self, index: &[usize]) -> usize {
        index.iter().zip(&self.strides).map(|(i, s)| i * s).sum()
    }

    /// Element at a logical index, converted to f32 (handles float16 outputs).
    #[inline]
    pub fn f32(&self, index: &[usize]) -> f32 {
        let o = self.offset(index);
        unsafe {
            match self.ty {
                MLMultiArrayDataType::Float32 => *(self.base as *const f32).add(o),
                MLMultiArrayDataType::Float16 => f16_to_f32(*(self.base as *const u16).add(o)),
                MLMultiArrayDataType::Double => *(self.base as *const f64).add(o) as f32,
                MLMultiArrayDataType::Int32 => *(self.base as *const i32).add(o) as f32,
                _ => 0.,
            }
        }
    }

    #[inline]
    pub fn i32(&self, index: &[usize]) -> i32 {
        if self.ty == MLMultiArrayDataType::Int32 {
            unsafe { *(self.base as *const i32).add(self.offset(index)) }
        } else {
            self.f32(index) as i32
        }
    }
}

fn f16_to_f32(h: u16) -> f32 {
    let sign = ((h >> 15) & 1) as u32;
    let exp = ((h >> 10) & 0x1f) as u32;
    let frac = (h & 0x3ff) as u32;
    let bits = match exp {
        0 if frac == 0 => sign << 31,
        0 => {
            // Subnormal: normalize.
            let mut e = 1i32;
            let mut f = frac;
            while f & 0x400 == 0 {
                f <<= 1;
                e -= 1;
            }
            (sign << 31) | (((e + 127 - 15) as u32) << 23) | ((f & 0x3ff) << 13)
        }
        0x1f => (sign << 31) | (0xff << 23) | (frac << 13),
        _ => (sign << 31) | ((exp + 127 - 15) << 23) | (frac << 13),
    };
    f32::from_bits(bits)
}
