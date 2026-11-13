//! Hand-written `#[repr(C)]` bindings for the parts of the OpenFX 1.4 C API that Milan Pro hosts (ofxCore.h,
//! ofxProperty.h, ofxImageEffect.h, ofxParam.h, ofxMemory.h, ofxMultiThread.h, ofxMessage.h, ofxProgress.h,
//! ofxTimeLine.h). The names and string values come from the OpenFX headers, Copyright (c) 2003-2015 The Open Effects
//! Association Ltd, BSD-3-Clause (see README.md, "OFX plug-ins"). Shared by `milan-plugin-host` and the test plug-in in
//! tools/ofx-test-plugin.
#![allow(dead_code, non_upper_case_globals, non_snake_case, clippy::upper_case_acronyms)]
use std::ffi::{CStr, c_char, c_int, c_uint, c_void};

pub type OfxStatus = c_int;
pub type OfxTime = f64;
pub type OfxPropertySetHandle = *mut c_void;
pub type OfxImageEffectHandle = *mut c_void;
pub type OfxImageClipHandle = *mut c_void;
pub type OfxParamSetHandle = *mut c_void;
pub type OfxParamHandle = *mut c_void;
pub type OfxImageMemoryHandle = *mut c_void;
pub type OfxMutexHandle = *mut c_void;

pub const kOfxStatOK: OfxStatus = 0;
pub const kOfxStatFailed: OfxStatus = 1;
pub const kOfxStatErrFatal: OfxStatus = 2;
pub const kOfxStatErrUnknown: OfxStatus = 3;
pub const kOfxStatErrMissingHostFeature: OfxStatus = 4;
pub const kOfxStatErrUnsupported: OfxStatus = 5;
pub const kOfxStatErrExists: OfxStatus = 6;
pub const kOfxStatErrFormat: OfxStatus = 7;
pub const kOfxStatErrMemory: OfxStatus = 8;
pub const kOfxStatErrBadHandle: OfxStatus = 9;
pub const kOfxStatErrBadIndex: OfxStatus = 10;
pub const kOfxStatErrValue: OfxStatus = 11;
pub const kOfxStatReplyYes: OfxStatus = 12;
pub const kOfxStatReplyNo: OfxStatus = 13;
pub const kOfxStatReplyDefault: OfxStatus = 14;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct OfxRectD {
    pub x1: f64,
    pub y1: f64,
    pub x2: f64,
    pub y2: f64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct OfxRangeD {
    pub min: f64,
    pub max: f64,
}

pub type FetchSuite = unsafe extern "C" fn(host: OfxPropertySetHandle, name: *const c_char, version: c_int) -> *const c_void;

#[repr(C)]
pub struct OfxHost {
    pub host: OfxPropertySetHandle,
    pub fetchSuite: FetchSuite,
}

pub type OfxPluginEntryPoint = unsafe extern "C" fn(action: *const c_char, handle: *const c_void, inArgs: OfxPropertySetHandle, outArgs: OfxPropertySetHandle) -> OfxStatus;

#[repr(C)]
pub struct OfxPlugin {
    pub pluginApi: *const c_char,
    pub apiVersion: c_int,
    pub pluginIdentifier: *const c_char,
    pub pluginVersionMajor: c_uint,
    pub pluginVersionMinor: c_uint,
    pub setHost: Option<unsafe extern "C" fn(host: *mut OfxHost)>,
    pub mainEntry: Option<OfxPluginEntryPoint>,
}

#[repr(C)]
pub struct OfxPropertySuiteV1 {
    pub propSetPointer: unsafe extern "C" fn(OfxPropertySetHandle, *const c_char, c_int, *mut c_void) -> OfxStatus,
    pub propSetString: unsafe extern "C" fn(OfxPropertySetHandle, *const c_char, c_int, *const c_char) -> OfxStatus,
    pub propSetDouble: unsafe extern "C" fn(OfxPropertySetHandle, *const c_char, c_int, f64) -> OfxStatus,
    pub propSetInt: unsafe extern "C" fn(OfxPropertySetHandle, *const c_char, c_int, c_int) -> OfxStatus,
    pub propSetPointerN: unsafe extern "C" fn(OfxPropertySetHandle, *const c_char, c_int, *const *mut c_void) -> OfxStatus,
    pub propSetStringN: unsafe extern "C" fn(OfxPropertySetHandle, *const c_char, c_int, *const *const c_char) -> OfxStatus,
    pub propSetDoubleN: unsafe extern "C" fn(OfxPropertySetHandle, *const c_char, c_int, *const f64) -> OfxStatus,
    pub propSetIntN: unsafe extern "C" fn(OfxPropertySetHandle, *const c_char, c_int, *const c_int) -> OfxStatus,
    pub propGetPointer: unsafe extern "C" fn(OfxPropertySetHandle, *const c_char, c_int, *mut *mut c_void) -> OfxStatus,
    pub propGetString: unsafe extern "C" fn(OfxPropertySetHandle, *const c_char, c_int, *mut *mut c_char) -> OfxStatus,
    pub propGetDouble: unsafe extern "C" fn(OfxPropertySetHandle, *const c_char, c_int, *mut f64) -> OfxStatus,
    pub propGetInt: unsafe extern "C" fn(OfxPropertySetHandle, *const c_char, c_int, *mut c_int) -> OfxStatus,
    pub propGetPointerN: unsafe extern "C" fn(OfxPropertySetHandle, *const c_char, c_int, *mut *mut c_void) -> OfxStatus,
    pub propGetStringN: unsafe extern "C" fn(OfxPropertySetHandle, *const c_char, c_int, *mut *mut c_char) -> OfxStatus,
    pub propGetDoubleN: unsafe extern "C" fn(OfxPropertySetHandle, *const c_char, c_int, *mut f64) -> OfxStatus,
    pub propGetIntN: unsafe extern "C" fn(OfxPropertySetHandle, *const c_char, c_int, *mut c_int) -> OfxStatus,
    pub propReset: unsafe extern "C" fn(OfxPropertySetHandle, *const c_char) -> OfxStatus,
    pub propGetDimension: unsafe extern "C" fn(OfxPropertySetHandle, *const c_char, *mut c_int) -> OfxStatus,
}

#[repr(C)]
pub struct OfxImageEffectSuiteV1 {
    pub getPropertySet: unsafe extern "C" fn(OfxImageEffectHandle, *mut OfxPropertySetHandle) -> OfxStatus,
    pub getParamSet: unsafe extern "C" fn(OfxImageEffectHandle, *mut OfxParamSetHandle) -> OfxStatus,
    pub clipDefine: unsafe extern "C" fn(OfxImageEffectHandle, *const c_char, *mut OfxPropertySetHandle) -> OfxStatus,
    pub clipGetHandle: unsafe extern "C" fn(OfxImageEffectHandle, *const c_char, *mut OfxImageClipHandle, *mut OfxPropertySetHandle) -> OfxStatus,
    pub clipGetPropertySet: unsafe extern "C" fn(OfxImageClipHandle, *mut OfxPropertySetHandle) -> OfxStatus,
    pub clipGetImage: unsafe extern "C" fn(OfxImageClipHandle, OfxTime, *const OfxRectD, *mut OfxPropertySetHandle) -> OfxStatus,
    pub clipReleaseImage: unsafe extern "C" fn(OfxPropertySetHandle) -> OfxStatus,
    pub clipGetRegionOfDefinition: unsafe extern "C" fn(OfxImageClipHandle, OfxTime, *mut OfxRectD) -> OfxStatus,
    pub abort: unsafe extern "C" fn(OfxImageEffectHandle) -> c_int,
    pub imageMemoryAlloc: unsafe extern "C" fn(OfxImageEffectHandle, usize, *mut OfxImageMemoryHandle) -> OfxStatus,
    pub imageMemoryFree: unsafe extern "C" fn(OfxImageMemoryHandle) -> OfxStatus,
    pub imageMemoryLock: unsafe extern "C" fn(OfxImageMemoryHandle, *mut *mut c_void) -> OfxStatus,
    pub imageMemoryUnlock: unsafe extern "C" fn(OfxImageMemoryHandle) -> OfxStatus,
}

/// The value functions are C variadic (`...`): callers pass their pointers or values after the fixed arguments.
#[repr(C)]
pub struct OfxParameterSuiteV1 {
    pub paramDefine: unsafe extern "C" fn(OfxParamSetHandle, *const c_char, *const c_char, *mut OfxPropertySetHandle) -> OfxStatus,
    pub paramGetHandle: unsafe extern "C" fn(OfxParamSetHandle, *const c_char, *mut OfxParamHandle, *mut OfxPropertySetHandle) -> OfxStatus,
    pub paramSetGetPropertySet: unsafe extern "C" fn(OfxParamSetHandle, *mut OfxPropertySetHandle) -> OfxStatus,
    pub paramGetPropertySet: unsafe extern "C" fn(OfxParamHandle, *mut OfxPropertySetHandle) -> OfxStatus,
    pub paramGetValue: unsafe extern "C" fn(OfxParamHandle, ...) -> OfxStatus,
    pub paramGetValueAtTime: unsafe extern "C" fn(OfxParamHandle, OfxTime, ...) -> OfxStatus,
    pub paramGetDerivative: unsafe extern "C" fn(OfxParamHandle, OfxTime, ...) -> OfxStatus,
    pub paramGetIntegral: unsafe extern "C" fn(OfxParamHandle, OfxTime, OfxTime, ...) -> OfxStatus,
    pub paramSetValue: unsafe extern "C" fn(OfxParamHandle, ...) -> OfxStatus,
    pub paramSetValueAtTime: unsafe extern "C" fn(OfxParamHandle, OfxTime, ...) -> OfxStatus,
    pub paramGetNumKeys: unsafe extern "C" fn(OfxParamHandle, *mut c_uint) -> OfxStatus,
    pub paramGetKeyTime: unsafe extern "C" fn(OfxParamHandle, c_uint, *mut OfxTime) -> OfxStatus,
    pub paramGetKeyIndex: unsafe extern "C" fn(OfxParamHandle, OfxTime, c_int, *mut c_int) -> OfxStatus,
    pub paramDeleteKey: unsafe extern "C" fn(OfxParamHandle, OfxTime) -> OfxStatus,
    pub paramDeleteAllKeys: unsafe extern "C" fn(OfxParamHandle) -> OfxStatus,
    pub paramCopy: unsafe extern "C" fn(OfxParamHandle, OfxParamHandle, OfxTime, *const OfxRangeD) -> OfxStatus,
    pub paramEditBegin: unsafe extern "C" fn(OfxParamSetHandle, *const c_char) -> OfxStatus,
    pub paramEditEnd: unsafe extern "C" fn(OfxParamSetHandle) -> OfxStatus,
}

#[repr(C)]
pub struct OfxMemorySuiteV1 {
    pub memoryAlloc: unsafe extern "C" fn(*mut c_void, usize, *mut *mut c_void) -> OfxStatus,
    pub memoryFree: unsafe extern "C" fn(*mut c_void) -> OfxStatus,
}

pub type OfxThreadFunctionV1 = unsafe extern "C" fn(c_uint, c_uint, *mut c_void);

#[repr(C)]
pub struct OfxMultiThreadSuiteV1 {
    pub multiThread: unsafe extern "C" fn(Option<OfxThreadFunctionV1>, c_uint, *mut c_void) -> OfxStatus,
    pub multiThreadNumCPUs: unsafe extern "C" fn(*mut c_uint) -> OfxStatus,
    pub multiThreadIndex: unsafe extern "C" fn(*mut c_uint) -> OfxStatus,
    pub multiThreadIsSpawnedThread: unsafe extern "C" fn() -> c_int,
    pub mutexCreate: unsafe extern "C" fn(*mut OfxMutexHandle, c_int) -> OfxStatus,
    pub mutexDestroy: unsafe extern "C" fn(OfxMutexHandle) -> OfxStatus,
    pub mutexLock: unsafe extern "C" fn(OfxMutexHandle) -> OfxStatus,
    pub mutexUnLock: unsafe extern "C" fn(OfxMutexHandle) -> OfxStatus,
    pub mutexTryLock: unsafe extern "C" fn(OfxMutexHandle) -> OfxStatus,
}

#[repr(C)]
pub struct OfxMessageSuiteV1 {
    pub message: unsafe extern "C" fn(*mut c_void, *const c_char, *const c_char, *const c_char, ...) -> OfxStatus,
}

#[repr(C)]
pub struct OfxMessageSuiteV2 {
    pub message: unsafe extern "C" fn(*mut c_void, *const c_char, *const c_char, *const c_char, ...) -> OfxStatus,
    pub setPersistentMessage: unsafe extern "C" fn(*mut c_void, *const c_char, *const c_char, *const c_char, ...) -> OfxStatus,
    pub clearPersistentMessage: unsafe extern "C" fn(*mut c_void) -> OfxStatus,
}

#[repr(C)]
pub struct OfxProgressSuiteV1 {
    pub progressStart: unsafe extern "C" fn(*mut c_void, *const c_char) -> OfxStatus,
    pub progressUpdate: unsafe extern "C" fn(*mut c_void, f64) -> OfxStatus,
    pub progressEnd: unsafe extern "C" fn(*mut c_void) -> OfxStatus,
}

#[repr(C)]
pub struct OfxProgressSuiteV2 {
    pub progressStart: unsafe extern "C" fn(*mut c_void, *const c_char, *const c_char) -> OfxStatus,
    pub progressUpdate: unsafe extern "C" fn(*mut c_void, f64) -> OfxStatus,
    pub progressEnd: unsafe extern "C" fn(*mut c_void) -> OfxStatus,
}

#[repr(C)]
pub struct OfxTimeLineSuiteV1 {
    pub getTime: unsafe extern "C" fn(*mut c_void, *mut f64) -> OfxStatus,
    pub gotoTime: unsafe extern "C" fn(*mut c_void, f64) -> OfxStatus,
    pub getTimeBounds: unsafe extern "C" fn(*mut c_void, *mut f64, *mut f64) -> OfxStatus,
}

macro_rules! names {
    ($($n:ident = $v:literal;)*) => { $(pub const $n: &CStr = $v;)* };
}

names! {
    // Suites
    kOfxPropertySuite = c"OfxPropertySuite";
    kOfxImageEffectSuite = c"OfxImageEffectSuite";
    kOfxParameterSuite = c"OfxParameterSuite";
    kOfxMemorySuite = c"OfxMemorySuite";
    kOfxMultiThreadSuite = c"OfxMultiThreadSuite";
    kOfxMessageSuite = c"OfxMessageSuite";
    kOfxProgressSuite = c"OfxProgressSuite";
    kOfxTimeLineSuite = c"OfxTimeLineSuite";
    kOfxImageEffectPluginApi = c"OfxImageEffectPluginAPI";
    // Actions
    kOfxActionLoad = c"OfxActionLoad";
    kOfxActionDescribe = c"OfxActionDescribe";
    kOfxActionUnload = c"OfxActionUnload";
    kOfxActionPurgeCaches = c"OfxActionPurgeCaches";
    kOfxActionCreateInstance = c"OfxActionCreateInstance";
    kOfxActionDestroyInstance = c"OfxActionDestroyInstance";
    kOfxActionInstanceChanged = c"OfxActionInstanceChanged";
    kOfxActionBeginInstanceChanged = c"OfxActionBeginInstanceChanged";
    kOfxActionEndInstanceChanged = c"OfxActionEndInstanceChanged";
    kOfxImageEffectActionDescribeInContext = c"OfxImageEffectActionDescribeInContext";
    kOfxImageEffectActionGetRegionOfDefinition = c"OfxImageEffectActionGetRegionOfDefinition";
    kOfxImageEffectActionGetRegionsOfInterest = c"OfxImageEffectActionGetRegionsOfInterest";
    kOfxImageEffectActionGetClipPreferences = c"OfxImageEffectActionGetClipPreferences";
    kOfxImageEffectActionIsIdentity = c"OfxImageEffectActionIsIdentity";
    kOfxImageEffectActionRender = c"OfxImageEffectActionRender";
    kOfxImageEffectActionBeginSequenceRender = c"OfxImageEffectActionBeginSequenceRender";
    kOfxImageEffectActionEndSequenceRender = c"OfxImageEffectActionEndSequenceRender";
    // Generic properties
    kOfxPropAPIVersion = c"OfxPropAPIVersion";
    kOfxPropType = c"OfxPropType";
    kOfxPropName = c"OfxPropName";
    kOfxPropLabel = c"OfxPropLabel";
    kOfxPropShortLabel = c"OfxPropShortLabel";
    kOfxPropLongLabel = c"OfxPropLongLabel";
    kOfxPropVersion = c"OfxPropVersion";
    kOfxPropVersionLabel = c"OfxPropVersionLabel";
    kOfxPropPluginDescription = c"OfxPropPluginDescription";
    kOfxPropTime = c"OfxPropTime";
    kOfxPropIsInteractive = c"OfxPropIsInteractive";
    kOfxPropInstanceData = c"OfxPropInstanceData";
    kOfxPropChangeReason = c"OfxPropChangeReason";
    kOfxPluginPropFilePath = c"OfxPluginPropFilePath";
    kOfxChangeUserEdited = c"OfxChangeUserEdited";
    kOfxChangePluginEdited = c"OfxChangePluginEdited";
    kOfxTypeImageEffectHost = c"OfxTypeImageEffectHost";
    kOfxTypeImageEffect = c"OfxTypeImageEffect";
    kOfxTypeImageEffectInstance = c"OfxTypeImageEffectInstance";
    kOfxTypeParameter = c"OfxTypeParameter";
    kOfxTypeParameterInstance = c"OfxTypeParameterInstance";
    kOfxTypeClip = c"OfxTypeClip";
    kOfxTypeImage = c"OfxTypeImage";
    // Image effect
    kOfxImageEffectPropSupportedContexts = c"OfxImageEffectPropSupportedContexts";
    kOfxImageEffectPropContext = c"OfxImageEffectPropContext";
    kOfxImageEffectContextFilter = c"OfxImageEffectContextFilter";
    kOfxImageEffectContextGeneral = c"OfxImageEffectContextGeneral";
    kOfxImageEffectPluginPropGrouping = c"OfxImageEffectPluginPropGrouping";
    kOfxImageEffectPropSupportedPixelDepths = c"OfxImageEffectPropSupportedPixelDepths";
    kOfxImageEffectPropSupportedComponents = c"OfxImageEffectPropSupportedComponents";
    kOfxImageEffectPropPixelDepth = c"OfxImageEffectPropPixelDepth";
    kOfxImageEffectPropComponents = c"OfxImageEffectPropComponents";
    kOfxImageClipPropUnmappedPixelDepth = c"OfxImageClipPropUnmappedPixelDepth";
    kOfxImageClipPropUnmappedComponents = c"OfxImageClipPropUnmappedComponents";
    kOfxImageEffectPropPreMultiplication = c"OfxImageEffectPropPreMultiplication";
    kOfxImageOpaque = c"OfxImageOpaque";
    kOfxImagePreMultiplied = c"OfxImageAlphaPremultiplied";
    kOfxBitDepthByte = c"OfxBitDepthByte";
    kOfxBitDepthShort = c"OfxBitDepthShort";
    kOfxBitDepthHalf = c"OfxBitDepthHalf";
    kOfxBitDepthFloat = c"OfxBitDepthFloat";
    kOfxImageComponentRGBA = c"OfxImageComponentRGBA";
    kOfxImageEffectPropSupportsMultipleClipDepths = c"OfxImageEffectPropSupportsMultipleClipDepths";
    kOfxImageEffectPropSupportsMultipleClipPARs = c"OfxImageEffectPropSupportsMultipleClipPARs";
    kOfxImageEffectPropSupportsMultiResolution = c"OfxImageEffectPropSupportsMultiResolution";
    kOfxImageEffectPropSupportsTiles = c"OfxImageEffectPropSupportsTiles";
    kOfxImageEffectPropTemporalClipAccess = c"OfxImageEffectPropTemporalClipAccess";
    kOfxImageEffectPropSupportsOverlays = c"OfxImageEffectPropSupportsOverlays";
    kOfxImageEffectHostPropIsBackground = c"OfxImageEffectHostPropIsBackground";
    kOfxImageEffectPropSetableFrameRate = c"OfxImageEffectPropSetableFrameRate";
    kOfxImageEffectPropSetableFielding = c"OfxImageEffectPropSetableFielding";
    kOfxImageEffectInstancePropSequentialRender = c"OfxImageEffectInstancePropSequentialRender";
    kOfxImageEffectPropSequentialRenderStatus = c"OfxImageEffectPropSequentialRenderStatus";
    kOfxImageEffectPropInteractiveRenderStatus = c"OfxImageEffectPropInteractiveRenderStatus";
    kOfxImageEffectPropRenderQualityDraft = c"OfxImageEffectPropRenderQualityDraft";
    kOfxImageEffectHostPropNativeOrigin = c"OfxImageEffectHostPropNativeOrigin";
    kOfxHostNativeOriginBottomLeft = c"kOfxImageEffectHostPropNativeOriginBottomLeft";
    kOfxImageEffectPropProjectSize = c"OfxImageEffectPropProjectSize";
    kOfxImageEffectPropProjectOffset = c"OfxImageEffectPropProjectOffset";
    kOfxImageEffectPropProjectExtent = c"OfxImageEffectPropProjectExtent";
    kOfxImageEffectPropProjectPixelAspectRatio = c"OfxImageEffectPropPixelAspectRatio";
    kOfxImageEffectInstancePropEffectDuration = c"OfxImageEffectInstancePropEffectDuration";
    kOfxImageEffectPropFrameRate = c"OfxImageEffectPropFrameRate";
    kOfxImageEffectPropFrameRange = c"OfxImageEffectPropFrameRange";
    kOfxImageEffectPropFrameStep = c"OfxImageEffectPropFrameStep";
    kOfxImageEffectPropUnmappedFrameRange = c"OfxImageEffectPropUnmappedFrameRange";
    kOfxImageEffectPropUnmappedFrameRate = c"OfxImageEffectPropUnmappedFrameRate";
    kOfxImageClipPropOptional = c"OfxImageClipPropOptional";
    kOfxImageClipPropIsMask = c"OfxImageClipPropIsMask";
    kOfxImageClipPropFieldOrder = c"OfxImageClipPropFieldOrder";
    kOfxImageClipPropConnected = c"OfxImageClipPropConnected";
    kOfxImageClipPropContinuousSamples = c"OfxImageClipPropContinuousSamples";
    kOfxImagePropPixelAspectRatio = c"OfxImagePropPixelAspectRatio";
    kOfxImagePropData = c"OfxImagePropData";
    kOfxImagePropBounds = c"OfxImagePropBounds";
    kOfxImagePropRegionOfDefinition = c"OfxImagePropRegionOfDefinition";
    kOfxImagePropRowBytes = c"OfxImagePropRowBytes";
    kOfxImagePropField = c"OfxImagePropField";
    kOfxImagePropUniqueIdentifier = c"OfxImagePropUniqueIdentifier";
    kOfxImageEffectPropRenderScale = c"OfxImageEffectPropRenderScale";
    kOfxImageFieldNone = c"OfxFieldNone";
    kOfxImageEffectPropFieldToRender = c"OfxImageEffectPropFieldToRender";
    kOfxImageEffectPropRenderWindow = c"OfxImageEffectPropRenderWindow";
    kOfxImageEffectPropRegionOfDefinition = c"OfxImageEffectPropRegionOfDefinition";
    kOfxImageEffectFrameVarying = c"OfxImageEffectFrameVarying";
    kOfxImageEffectOutputClipName = c"Output";
    kOfxImageEffectSimpleSourceClipName = c"Source";
    // Parameters
    kOfxParamTypeInteger = c"OfxParamTypeInteger";
    kOfxParamTypeDouble = c"OfxParamTypeDouble";
    kOfxParamTypeBoolean = c"OfxParamTypeBoolean";
    kOfxParamTypeChoice = c"OfxParamTypeChoice";
    kOfxParamTypeRGBA = c"OfxParamTypeRGBA";
    kOfxParamTypeRGB = c"OfxParamTypeRGB";
    kOfxParamTypeDouble2D = c"OfxParamTypeDouble2D";
    kOfxParamTypeInteger2D = c"OfxParamTypeInteger2D";
    kOfxParamTypeDouble3D = c"OfxParamTypeDouble3D";
    kOfxParamTypeInteger3D = c"OfxParamTypeInteger3D";
    kOfxParamTypeString = c"OfxParamTypeString";
    kOfxParamTypeCustom = c"OfxParamTypeCustom";
    kOfxParamTypeGroup = c"OfxParamTypeGroup";
    kOfxParamTypePage = c"OfxParamTypePage";
    kOfxParamTypePushButton = c"OfxParamTypePushButton";
    kOfxParamPropType = c"OfxParamPropType";
    kOfxParamPropDefault = c"OfxParamPropDefault";
    kOfxParamPropMin = c"OfxParamPropMin";
    kOfxParamPropMax = c"OfxParamPropMax";
    kOfxParamPropDisplayMin = c"OfxParamPropDisplayMin";
    kOfxParamPropDisplayMax = c"OfxParamPropDisplayMax";
    kOfxParamPropChoiceOption = c"OfxParamPropChoiceOption";
    kOfxParamPropParent = c"OfxParamPropParent";
    kOfxParamPropSecret = c"OfxParamPropSecret";
    kOfxParamPropEnabled = c"OfxParamPropEnabled";
    kOfxParamPropHint = c"OfxParamPropHint";
    kOfxParamPropScriptName = c"OfxParamPropScriptName";
    kOfxParamPropStringMode = c"OfxParamPropStringMode";
    kOfxParamStringIsSingleLine = c"OfxParamStringIsSingleLine";
    kOfxParamStringIsMultiLine = c"OfxParamStringIsMultiLine";
    kOfxParamPropAnimates = c"OfxParamPropAnimates";
    kOfxParamPropIsAnimating = c"OfxParamPropIsAnimating";
    kOfxParamPropPersistant = c"OfxParamPropPersistant";
    kOfxParamPropEvaluateOnChange = c"OfxParamPropEvaluateOnChange";
    kOfxParamPropDoubleType = c"OfxParamPropDoubleType";
    kOfxParamDoubleTypePlain = c"OfxParamDoubleTypePlain";
    kOfxParamPropGroupOpen = c"OfxParamPropGroupOpen";
    kOfxParamHostPropSupportsCustomAnimation = c"OfxParamHostPropSupportsCustomAnimation";
    kOfxParamHostPropSupportsStringAnimation = c"OfxParamHostPropSupportsStringAnimation";
    kOfxParamHostPropSupportsBooleanAnimation = c"OfxParamHostPropSupportsBooleanAnimation";
    kOfxParamHostPropSupportsChoiceAnimation = c"OfxParamHostPropSupportsChoiceAnimation";
    kOfxParamHostPropSupportsCustomInteract = c"OfxParamHostPropSupportsCustomInteract";
    kOfxParamHostPropSupportsParametricAnimation = c"OfxParamHostPropSupportsParametricAnimation";
    kOfxParamHostPropMaxParameters = c"OfxParamHostPropMaxParameters";
    kOfxParamHostPropMaxPages = c"OfxParamHostPropMaxPages";
    kOfxParamHostPropPageRowColumnCount = c"OfxParamHostPropPageRowColumnCount";
    // Messages
    kOfxMessageError = c"OfxMessageError";
    kOfxMessageFatal = c"OfxMessageFatal";
    kOfxMessageQuestion = c"OfxMessageQuestion";
}
