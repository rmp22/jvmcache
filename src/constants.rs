pub const CACHE_PROTOCOL_VERSION: &str = "v3";

pub const DIR_OBJECTS: &str = "objects";
pub const DIR_CAS: &str = "cas";
pub const DIR_TMP: &str = "tmp";
pub const DIR_BASELINES: &str = "baselines";
pub const DIR_CDS: &str = "cds";

pub const FILE_MANIFEST: &str = "manifest.json";
pub const FILE_STATS: &str = "stats.json";
pub const FILE_ACTIVITY_LOG: &str = "activity.log";
pub const FILE_ACTIVITY_LOG_OLD: &str = "activity.log.old";
pub const FILE_FINGERPRINTS: &str = "compiler_fingerprints.json";
pub const FILE_DAEMON_SOCK: &str = "daemon.sock";
pub const FILE_DAEMON_JAR: &str = "daemon/jvmcache-daemon.jar";

pub const COMPILER_CMD_JAVAC: &str = "javac";
pub const COMPILER_CMD_KOTLINC: &str = "kotlinc";
pub const COMPILER_CMD_KOTLINC_JVM: &str = "kotlinc-jvm";
pub const COMPILER_CMD_KAPT: &str = "kapt";
pub const COMPILER_CMD_D8: &str = "d8";
pub const COMPILER_CMD_R8: &str = "r8";

pub const FLAG_VERSION: &str = "-version";
pub const FLAG_VERSION_GNU: &str = "--version";

pub const TAG_CLASSES: &str = "classes";
pub const TAG_ANNO: &str = "anno";
pub const TAG_HEADERS: &str = "headers";
pub const TAG_ABI_HEADERS: &str = "abi_headers";
pub const TAG_KAPT_STUBS: &str = "kapt_stubs";
pub const TAG_KAPT_SOURCES: &str = "kapt_sources";
pub const TAG_KAPT_CLASSES: &str = "kapt_classes";
pub const TAG_DEX: &str = "dex";
pub const TAG_DEX_PACKAGES: &str = "dex_packages";
pub const TAG_DEX_GLOBALS: &str = "dex_globals";
pub const TAG_R8_DICT: &str = "r8_dict";
pub const TAG_R8_CONFIG: &str = "r8_config";
pub const TAG_R8_USAGE: &str = "r8_usage";
pub const TAG_R8_DEPS: &str = "r8_deps";

pub const DEFAULT_MAX_CACHE_SIZE_MB: u64 = 5120;
pub const CACHE_CLEANUP_WATERMARK_RATIO: f64 = 0.8;
pub const MAX_ACTIVITY_LOG_BYTES: u64 = 20 * 1024 * 1024;

pub const MAX_DELTA_MODIFIED_FILES: usize = 50;
pub const MAX_DELTA_DELETED_FILES: usize = 20;
pub const MAX_TARGETED_CALLERS: usize = 50;

pub const HASH_BUFFER_SIZE: usize = 65536;
pub const PARALLEL_HASH_MIN_FILES: usize = 4;
pub const PARALLEL_HASH_MAX_THREADS: usize = 16;
pub const PARALLEL_STORE_MIN_FILES: usize = 8;

pub const MAX_ARGFILE_DEPTH: usize = 10;
pub const DAEMON_IPC_MAX_BYTES: usize = 32 * 1024 * 1024;
pub const DAEMON_CONNECT_RETRIES: usize = 60;
pub const DAEMON_CONNECT_INTERVAL_MS: u64 = 50;
pub const DAEMON_READ_TIMEOUT_SECS: u64 = 300;
pub const DAEMON_WRITE_TIMEOUT_SECS: u64 = 30;

pub const AOSP_MARKER_SOONG: &str = "build/soong";
pub const AOSP_MARKER_JDK: &str = "prebuilts/jdk";
pub const EXCLUDE_PATH_INTERPOSER: &str = "path_interposer";
pub const EXCLUDE_JVMCACHE: &str = "jvmcache";

pub const AOSP_JAVAC_JDK21: &str = "prebuilts/jdk/jdk21/linux-x86/bin/javac";
pub const AOSP_JAVAC_JDK25: &str = "prebuilts/jdk/jdk25/linux-x86/bin/javac";
pub const AOSP_JAVA_JDK21: &str = "prebuilts/jdk/jdk21/linux-x86/bin/java";
pub const AOSP_KOTLINC_BIN: &str = "external/kotlinc/bin/kotlinc";
pub const AOSP_KOTLINC_HOME: &str = "external/kotlinc";
pub const AOSP_KOTLIN_COMPILER_JAR: &str = "lib/kotlin-compiler.jar";
pub const AOSP_KOTLIN_ABI_GEN_JAR: &str = "external/kotlinc/lib/jvm-abi-gen.jar";
pub const AOSP_KOTLIN_KAPT_JAR: &str = "external/kotlinc/lib/kotlin-annotation-processing.jar";

pub const AOSP_HOST_D8: &str = "out/host/linux-x86/bin/d8";
pub const AOSP_PREBUILT_D8: &str = "prebuilts/cmdline-tools/tools/bin/d8";
pub const AOSP_R8_D8: &str = "prebuilts/r8/d8";
pub const AOSP_HOST_R8: &str = "out/host/linux-x86/bin/r8";
pub const AOSP_PREBUILT_R8: &str = "prebuilts/r8/r8";
pub const AOSP_R8_JAR: &str = "prebuilts/r8/r8.jar";

pub const AOSP_INTERMEDIATES_MARKER: &str = ".intermediates/";
pub const AOSP_ANDROID_COMMON_MARKER: &str = "/android_common";
pub const FALLBACK_GENERIC_TARGET: &str = "generic-target";

pub const CDS_ARCHIVE_KOTLINC: &str = "kotlinc.jsa";
pub const CDS_ARCHIVE_JAVAC: &str = "javac.jsa";
pub const CDS_ARCHIVE_D8: &str = "d8.jsa";
pub const CDS_ARCHIVE_R8: &str = "r8.jsa";

pub const FLAG_FAST_JAR_FS: &str = "-Xfast-jar-fs";
pub const FLAG_COMPILE_POLICY_SIMPLE: &str = "-XDcompilePolicy=simple";
pub const FLAG_COMPILE_POLICY_PREFIX: &str = "-XDcompilePolicy=";
pub const FLAG_PROC_NONE: &str = "-proc:none";
pub const FLAG_PROC_PREFIX: &str = "-proc:";
pub const FLAG_BACKEND_THREADS_PREFIX: &str = "-Xbackend-threads=";
pub const FLAG_MULTI_PLATFORM: &str = "-Xmulti-platform";
pub const FLAG_EXPECT_ACTUAL_CLASSES: &str = "-Xexpect-actual-classes";
pub const FLAG_PROCESSOR: &str = "-processor";
pub const FLAG_PROCESSOR_PATH: &str = "-processorpath";
pub const FLAG_PROCESSOR_PATH_LONG: &str = "--processor-path";
pub const FLAG_PROCESSOR_MODULE_PATH: &str = "--processor-module-path";

pub const PLUGIN_MARKER_JVM_ABI_GEN: &str = "jvm-abi-gen";
pub const PLUGIN_MARKER_ANNO_PROC: &str = "annotation-processing";

pub const XML_TAG_MODULE_OPEN: &str = "<module ";
pub const XML_TAG_SOURCES_OPEN: &str = "<sources ";
pub const XML_TAG_JAVA_ROOTS_OPEN: &str = "<javaSourceRoots ";
pub const XML_TAG_CLASSPATH_OPEN: &str = "<classpath ";
pub const XML_TAG_FRIEND_DIR_OPEN: &str = "<friendDir ";
pub const XML_ATTR_OUTPUT_DIR: &str = "outputDir";
pub const XML_ATTR_PATH: &str = "path";

pub const EXT_KT: &str = "kt";
pub const EXT_KTS: &str = "kts";
pub const EXT_JAVA: &str = "java";
pub const EXT_CLASS: &str = "class";
pub const EXT_JAR: &str = "jar";
pub const EXT_ZIP: &str = "zip";
pub const EXT_DEX: &str = "dex";
pub const EXT_APK: &str = "apk";
pub const EXT_XML: &str = "xml";
pub const EXT_RSP: &str = "rsp";
pub const EXT_JSON: &str = "json";
pub const EXT_KOTLIN_MODULE: &str = "kotlin_module";
pub const DOT_EXT_KT: &str = ".kt";
pub const DOT_EXT_JAVA: &str = ".java";

pub const FLAG_BUILD_FILE: &str = "-build-file";
pub const FLAG_BUILD_FILE_PREFIX: &str = "-build-file=";
pub const FLAG_XBUILD_FILE: &str = "-Xbuild-file";
pub const FLAG_XBUILD_FILE_PREFIX: &str = "-Xbuild-file=";
pub const FLAG_FRIEND_PATHS_PREFIX: &str = "-Xfriend-paths=";
pub const FLAG_CP: &str = "-cp";
pub const FLAG_CLASSPATH: &str = "-classpath";
pub const FLAG_CLASS_PATH: &str = "--class-path";
pub const FLAG_BOOTCLASSPATH: &str = "-bootclasspath";
pub const FLAG_DIR: &str = "-d";
pub const FLAG_SRC_DIR: &str = "-s";
pub const FLAG_HEADER_DIR: &str = "-h";
pub const FLAG_OUTPUT_SHORT: &str = "-o";
pub const FLAG_OUTPUT_LONG: &str = "--output";
pub const FLAG_PACKAGE_OUTPUT: &str = "--package-output";
pub const FLAG_PRINT_MAPPING: &str = "-printmapping";
pub const FLAG_PRINT_CONFIGURATION: &str = "-printconfiguration";
pub const FLAG_PRINT_USAGE: &str = "-printusage";
pub const FLAG_DEPS_FILE: &str = "--deps-file";
pub const FLAG_GLOBALS_OUTPUT: &str = "--globals-output";
pub const FLAG_NO_DEX_INPUT_JAR: &str = "--no-dex-input-jar";
pub const FLAG_INJARS: &str = "-injars";
pub const FLAG_SOCKET: &str = "--socket";

pub const MAIN_CLASS_KOTLINC: &str = "org.jetbrains.kotlin.cli.jvm.K2JVMCompiler";
pub const DAEMON_MAIN_CLASS: &str = "org.jvmcache.daemon.WorkerMain";

pub const COMPILER_ERROR_MARKER: &str = "error:";
pub const SUBDIR_STUBS: &str = "stubs";
pub const DIR_META_INF: &str = "META-INF";
pub const DIR_KOTLIN: &str = "kotlin";
pub const DIR_LIBS: &str = "libs";
pub const DIR_OUT: &str = "out";

pub const PLUGIN_MARKER_JVM_ABI: &str = "jvm.abi";
pub const PLUGIN_ATTR_OUTPUT_DIR: &str = "outputDir=";
pub const PLUGIN_PREFIX_KAPT_STUBS: &str = "kapt3:stubs=";
pub const PLUGIN_PREFIX_KAPT_SOURCES: &str = "kapt3:sources=";
pub const PLUGIN_PREFIX_KAPT_CLASSES: &str = "kapt3:classes=";
pub const PLUGIN_KEY_STRIP_METADATA: &str = "stripMetadata";
pub const PLUGIN_OPT_STRIP_METADATA: &str = "plugin:org.jetbrains.kotlin.kapt3:stripMetadata=true";
pub const PLUGIN_PREFIX_COMPILED_SOURCES: &str = "plugin:org.jetbrains.kotlin.kapt3:compiledSourcesDir=";
pub const FLAG_P: &str = "-P";

pub const KAPT_ERROR_OBJECT_ANNOTATION: &str = "@java.lang.Object()";
pub const KAPT_ERROR_UNRESOLVED_MARKER: &str = "could not resolve ";
pub const KAPT_ERROR_NON_EXISTENT_CLASS: &str = "NonExistentClass";
pub const KAPT_PACKAGE_JVM_FUNCTIONS: &str = "kotlin.jvm.functions";
