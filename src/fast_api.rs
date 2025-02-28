use crate::support::Opaque;
use libc::c_void;
use std::{
  mem::align_of,
  ptr::{self, NonNull},
};

extern "C" {
  fn v8__CTypeInfo__New(ty: CType) -> *mut CTypeInfo;
  fn v8__CTypeInfo__New__From__Slice(
    len: usize,
    tys: *const CTypeSequenceInfo,
  ) -> *mut CTypeInfo;
  fn v8__CFunctionInfo__New(
    return_info: *const CTypeInfo,
    args_len: usize,
    args_info: *const CTypeInfo,
  ) -> *mut CFunctionInfo;
}

#[repr(C)]
#[derive(Default)]
pub struct CFunctionInfo(Opaque);

#[repr(C)]
#[derive(Default)]
pub struct CFunction(Opaque);

impl CFunctionInfo {
  pub(crate) unsafe fn new(
    args: *const CTypeInfo,
    args_len: usize,
    return_type: *const CTypeInfo,
  ) -> NonNull<CFunctionInfo> {
    NonNull::new_unchecked(v8__CFunctionInfo__New(return_type, args_len, args))
  }
}

#[repr(C)]
#[derive(Debug)]
pub struct CTypeInfo(Opaque);

impl CTypeInfo {
  pub(crate) fn new(ty: CType) -> NonNull<CTypeInfo> {
    unsafe { NonNull::new_unchecked(v8__CTypeInfo__New(ty)) }
  }

  pub(crate) fn new_from_slice(types: &[Type]) -> NonNull<CTypeInfo> {
    let mut structs = vec![];

    for type_ in types.iter() {
      structs.push(type_.into())
    }

    unsafe {
      NonNull::new_unchecked(v8__CTypeInfo__New__From__Slice(
        structs.len(),
        structs.as_ptr(),
      ))
    }
  }
}

#[derive(Clone, Copy, PartialEq, Debug)]
#[repr(u8)]
pub enum SequenceType {
  Scalar,
  /// sequence<T>
  IsSequence,
  /// TypedArray of T or any ArrayBufferView if T is void
  IsTypedArray,
  /// ArrayBuffer
  IsArrayBuffer,
}

#[derive(Clone, Copy)]
#[repr(u8)]
#[non_exhaustive]
pub enum CType {
  Void = 0,
  Bool,
  Uint8,
  Int32,
  Uint32,
  Int64,
  Uint64,
  Float32,
  Float64,
  V8Value,
  // https://github.com/v8/v8/blob/492a32943bc34a527f42df2ae15a77154b16cc84/include/v8-fast-api-calls.h#L264-L267
  // kCallbackOptionsType is not part of the Type enum
  // because it is only used internally. Use value 255 that is larger
  // than any valid Type enum.
  CallbackOptions = 255,
}

#[derive(Clone, Copy)]
#[non_exhaustive]
pub enum Type {
  Void,
  Bool,
  Int32,
  Uint32,
  Int64,
  Uint64,
  Float32,
  Float64,
  V8Value,
  CallbackOptions,
  Sequence(CType),
  TypedArray(CType),
  ArrayBuffer(CType),
}

impl From<&Type> for CType {
  fn from(ty: &Type) -> CType {
    match ty {
      Type::Void => CType::Void,
      Type::Bool => CType::Bool,
      Type::Int32 => CType::Int32,
      Type::Uint32 => CType::Uint32,
      Type::Int64 => CType::Int64,
      Type::Uint64 => CType::Uint64,
      Type::Float32 => CType::Float32,
      Type::Float64 => CType::Float64,
      Type::V8Value => CType::V8Value,
      Type::CallbackOptions => CType::CallbackOptions,
      Type::Sequence(ty) => *ty,
      Type::TypedArray(ty) => *ty,
      Type::ArrayBuffer(ty) => *ty,
    }
  }
}

impl From<&Type> for SequenceType {
  fn from(ty: &Type) -> SequenceType {
    match ty {
      Type::Sequence(_) => SequenceType::IsSequence,
      Type::TypedArray(_) => SequenceType::IsTypedArray,
      Type::ArrayBuffer(_) => SequenceType::IsArrayBuffer,
      _ => SequenceType::Scalar,
    }
  }
}

impl From<&Type> for CTypeSequenceInfo {
  fn from(ty: &Type) -> CTypeSequenceInfo {
    CTypeSequenceInfo {
      c_type: ty.into(),
      sequence_type: ty.into(),
    }
  }
}

#[repr(C)]
struct CTypeSequenceInfo {
  c_type: CType,
  sequence_type: SequenceType,
}

#[repr(C)]
pub union FastApiCallbackData {
  /// `data_ptr` allows for default constructing FastApiCallbackOptions.
  pub data_ptr: *mut c_void,
  /// The `data` passed to the FunctionTemplate constructor, or `undefined`.
  pub data: crate::Value,
}

/// A struct which may be passed to a fast call callback, like so
/// ```c
/// void FastMethodWithOptions(int param, FastApiCallbackOptions& options);
/// ```
#[repr(C)]
pub struct FastApiCallbackOptions {
  /// If the callback wants to signal an error condition or to perform an
  /// allocation, it must set options.fallback to true and do an early return
  /// from the fast method. Then V8 checks the value of options.fallback and if
  /// it's true, falls back to executing the SlowCallback, which is capable of
  /// reporting the error (either by throwing a JS exception or logging to the
  /// console) or doing the allocation. It's the embedder's responsibility to
  /// ensure that the fast callback is idempotent up to the point where error and
  /// fallback conditions are checked, because otherwise executing the slow
  /// callback might produce visible side-effects twice.
  pub fallback: bool,
  pub data: FastApiCallbackData,
  /// When called from WebAssembly, a view of the calling module's memory.
  pub wasm_memory: *const FastApiTypedArray<u8>,
}

// https://source.chromium.org/chromium/chromium/src/+/main:v8/include/v8-fast-api-calls.h;l=336
#[repr(C)]
pub struct FastApiTypedArray<T: Default> {
  pub byte_length: usize,
  // This pointer should include the typed array offset applied.
  // It's not guaranteed that it's aligned to sizeof(T), it's only
  // guaranteed that it's 4-byte aligned, so for 8-byte types we need to
  // provide a special implementation for reading from it, which hides
  // the possibly unaligned read in the `get` method.
  data: *mut T,
}

impl<T: Default> FastApiTypedArray<T> {
  #[inline]
  pub fn get(&self, index: usize) -> T {
    debug_assert!(index < self.byte_length);
    let mut t: T = Default::default();
    unsafe {
      ptr::copy_nonoverlapping(self.data.add(index), &mut t, 1);
    }
    t
  }

  #[inline]
  pub fn get_storage_if_aligned(&self) -> Option<&mut [T]> {
    if (self.data as usize) % align_of::<T>() != 0 {
      return None;
    }
    Some(unsafe {
      std::slice::from_raw_parts_mut(
        self.data,
        self.byte_length / align_of::<T>(),
      )
    })
  }
}

pub trait FastFunction {
  fn args(&self) -> &'static [Type] {
    &[]
  }
  fn return_type(&self) -> CType {
    CType::Void
  }
  fn function(&self) -> *const c_void;
}
