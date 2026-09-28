package org.jvmcache.daemon;

import java.io.*;
import java.lang.reflect.Method;
import java.net.StandardProtocolFamily;
import java.net.UnixDomainSocketAddress;
import java.nio.ByteBuffer;
import java.nio.channels.ServerSocketChannel;
import java.nio.channels.SocketChannel;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.Paths;
import java.nio.file.attribute.PosixFilePermissions;
import java.util.ArrayList;
import java.util.List;
import java.util.Set;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;
import javax.tools.JavaCompiler;
import javax.tools.ToolProvider;

public class WorkerMain {
    private static final String FLAG_JVM_TARGET = "-jvm-target";
    private static final String FLAG_SOURCE = "-source";
    private static final String FLAG_TARGET = "-target";
    private static final String FLAG_RELEASE = "-release";
    private static final String FLAG_ENCODING = "-encoding";
    private static final String FLAG_LANGUAGE_VERSION = "-language-version";
    private static final String FLAG_API_VERSION = "-api-version";
    private static final String FLAG_OPT_IN = "-opt-in";
    private static final String FLAG_MODULE_NAME = "-module-name";
    private static final String FLAG_XMAXERRS = "-Xmaxerrs";
    private static final String FLAG_XMAXWARNS = "-Xmaxwarns";

    private static final Set<String> NON_PATH_OPTIONS = Set.of(
        FLAG_JVM_TARGET,
        FLAG_SOURCE,
        FLAG_TARGET,
        FLAG_RELEASE,
        FLAG_ENCODING,
        FLAG_LANGUAGE_VERSION,
        FLAG_API_VERSION,
        FLAG_OPT_IN,
        FLAG_MODULE_NAME,
        FLAG_XMAXERRS,
        FLAG_XMAXWARNS
    );

    private static final Set<String> PATH_OPTIONS = Set.of(
        "-d",
        "-s",
        "-h",
        "-o",
        "--output",
        "--package-output",
        "-printmapping",
        "-printconfiguration",
        "-printusage",
        "--deps-file",
        "--globals-output",
        "-injars",
        "-libraryjars",
        "--lib",
        "--classpath",
        "--pg-conf",
        "--packages",
        "--mod-packages",
        "--main-dex-rules",
        "--main-dex-list",
        "--globals"
    );

    private static final Set<String> CLASSPATH_OPTIONS = Set.of(
        "-cp",
        "-classpath",
        "--class-path"
    );

    private static boolean isNonPathOptionWithValue(String arg) {
        return NON_PATH_OPTIONS.contains(arg);
    }

    private static boolean isPathOptionWithValue(String arg) {
        return PATH_OPTIONS.contains(arg);
    }

    private static boolean isClasspathOption(String arg) {
        return CLASSPATH_OPTIONS.contains(arg);
    }
    public static void main(String[] args) throws Exception {
        String socketPath = null;
        for (int i = 0; i < args.length; i++) {
            if ("--socket".equals(args[i]) && i + 1 < args.length) {
                socketPath = args[i + 1];
                i++;
            }
        }

        if (socketPath == null) {
            socketPath = System.getProperty("jvmcache.socket", "/tmp/jvmcache-" + System.getProperty("user.name") + ".sock");
        }

        Path sockFile = Paths.get(socketPath);
        if (Files.exists(sockFile)) {
            Files.delete(sockFile);
        }
        if (sockFile.getParent() != null) {
            Files.createDirectories(sockFile.getParent());
        }

        int workers = Math.max(2, Runtime.getRuntime().availableProcessors());
        ExecutorService threadPool = Executors.newFixedThreadPool(workers, r -> {
            Thread t = new Thread(r, "jvmcache-worker");
            t.setDaemon(true);
            return t;
        });

        UnixDomainSocketAddress address = UnixDomainSocketAddress.of(sockFile);
        try (ServerSocketChannel server = ServerSocketChannel.open(StandardProtocolFamily.UNIX)) {
            server.bind(address);
            try {
                Files.setPosixFilePermissions(sockFile, PosixFilePermissions.fromString("rwxrwxrwx"));
            } catch (Exception e) {
                System.err.println("jvmcache-daemon: warning: could not set socket permissions: " + e.getMessage());
            }
            sockFile.toFile().deleteOnExit();

            while (true) {
                try {
                    SocketChannel client = server.accept();
                    threadPool.submit(() -> {
                        try (client) {
                            handleClient(client);
                        } catch (Exception e) {
                            System.err.println("jvmcache-daemon: error handling request: " + e.getMessage());
                        }
                    });
                } catch (Exception e) {
                    if (server.isOpen()) {
                        System.err.println("jvmcache-daemon: accept error: " + e.getMessage());
                    } else {
                        break;
                    }
                }
            }
        }
    }

    private static void handleClient(SocketChannel client) throws Exception {
        ByteBuffer lenBuf = ByteBuffer.allocate(4);
        while (lenBuf.hasRemaining()) {
            if (client.read(lenBuf) == -1) return;
        }
        lenBuf.flip();
        int payloadLen = lenBuf.getInt();
        if (payloadLen <= 0 || payloadLen > 10 * 1024 * 1024) return;

        ByteBuffer payload = ByteBuffer.allocate(payloadLen);
        while (payload.hasRemaining()) {
            if (client.read(payload) == -1) return;
        }
        payload.flip();
        String json = StandardCharsets.UTF_8.decode(payload).toString();

        String compiler = extractJsonField(json, "compiler");
        String workingDir = extractJsonField(json, "working_dir");
        List<String> argsList = extractJsonArray(json, "args");

        if (workingDir != null && !workingDir.isEmpty()) {
            System.setProperty("user.dir", workingDir);
        }

        int exitCode = 1;
        String stdout = "";
        String stderr = "";

        if ("javac".equalsIgnoreCase(compiler)) {
            ByteArrayOutputStream out = new ByteArrayOutputStream();
            ByteArrayOutputStream err = new ByteArrayOutputStream();
            JavaCompiler javac = ToolProvider.getSystemJavaCompiler();
            if (javac != null) {
                List<String> adjustedArgs = adjustArgs(argsList, workingDir);
                exitCode = javac.run(null, out, err, adjustedArgs.toArray(new String[0]));
            } else {
                err.write("ToolProvider.getSystemJavaCompiler() returned null".getBytes(StandardCharsets.UTF_8));
                exitCode = 2;
            }
            stdout = out.toString(StandardCharsets.UTF_8);
            stderr = err.toString(StandardCharsets.UTF_8);
        } else if ("kotlinc".equalsIgnoreCase(compiler)) {
            ByteArrayOutputStream err = new ByteArrayOutputStream();
            PrintStream errPs = new PrintStream(err);
            try {
                Class<?> cls = Class.forName("org.jetbrains.kotlin.cli.jvm.K2JVMCompiler");
                Object compilerInst = cls.getDeclaredConstructor().newInstance();
                Method execMethod = cls.getMethod("exec", PrintStream.class, String[].class);
                List<String> adjustedArgs = adjustArgs(argsList, workingDir);
                Object res = execMethod.invoke(compilerInst, errPs, adjustedArgs.toArray(new String[0]));
                if (res != null && "OK".equals(res.toString())) {
                    exitCode = 0;
                } else {
                    exitCode = 1;
                }
            } catch (Exception e) {
                e.printStackTrace(errPs);
                exitCode = 1;
            }
            stderr = err.toString(StandardCharsets.UTF_8);
        } else if ("d8".equalsIgnoreCase(compiler) || "r8".equalsIgnoreCase(compiler)) {
            ByteArrayOutputStream out = new ByteArrayOutputStream();
            ByteArrayOutputStream err = new ByteArrayOutputStream();
            PrintStream outPs = new PrintStream(out);
            PrintStream errPs = new PrintStream(err);
            try {
                String className = "d8".equalsIgnoreCase(compiler) ? "com.android.tools.r8.D8" : "com.android.tools.r8.R8";
                Class<?> cls = Class.forName(className);
                Method mainMethod = cls.getMethod("main", String[].class);
                List<String> adjustedArgs = adjustArgs(argsList, workingDir);
                synchronized (PROCESS_LOCK) {
                    PrintStream oldOut = System.out;
                    PrintStream oldErr = System.err;
                    try {
                        System.setOut(outPs);
                        System.setErr(errPs);
                        mainMethod.invoke(null, (Object) adjustedArgs.toArray(new String[0]));
                        exitCode = 0;
                    } finally {
                        System.setOut(oldOut);
                        System.setErr(oldErr);
                    }
                }
            } catch (Exception e) {
                exitCode = 1;
                e.printStackTrace(errPs);
            }
            stdout = out.toString(StandardCharsets.UTF_8);
            stderr = err.toString(StandardCharsets.UTF_8);
        }

        String respJson = "{\"exit_code\":" + exitCode
                + ",\"stdout\":\"" + escapeJson(stdout) + "\""
                + ",\"stderr\":\"" + escapeJson(stderr) + "\"}";
        byte[] respBytes = respJson.getBytes(StandardCharsets.UTF_8);

        ByteBuffer outBuf = ByteBuffer.allocate(4 + respBytes.length);
        outBuf.putInt(respBytes.length);
        outBuf.put(respBytes);
        outBuf.flip();
        while (outBuf.hasRemaining()) {
            client.write(outBuf);
        }
    }

    private static List<String> adjustArgs(List<String> args, String workingDir) {
        Path base = (workingDir != null && !workingDir.isEmpty()) ? Paths.get(workingDir) : null;
        List<String> result = new ArrayList<>();
        for (int i = 0; i < args.size(); i++) {
            String arg = args.get(i);
            if (arg.startsWith("-J")) {
                continue;
            }
            if (base == null) {
                result.add(arg);
                continue;
            }
            if (isNonPathOptionWithValue(arg) && i + 1 < args.size()) {
                result.add(arg);
                result.add(args.get(i + 1));
                i++;
                continue;
            }
            if (isPathOptionWithValue(arg) && i + 1 < args.size()) {
                result.add(arg);
                Path dPath = Paths.get(args.get(i + 1));
                if (!dPath.isAbsolute()) {
                    dPath = base.resolve(dPath);
                }
                result.add(dPath.toString());
                i++;
            } else if (isClasspathOption(arg) && i + 1 < args.size()) {
                result.add(arg);
                String cp = args.get(i + 1);
                String[] parts = cp.split(":");
                StringBuilder resolvedCp = new StringBuilder();
                for (int pIdx = 0; pIdx < parts.length; pIdx++) {
                    if (pIdx > 0) resolvedCp.append(":");
                    Path p = Paths.get(parts[pIdx]);
                    if (!p.isAbsolute()) {
                        p = base.resolve(p);
                    }
                    resolvedCp.append(p.toString());
                }
                result.add(resolvedCp.toString());
                i++;
            } else if (arg.startsWith("-Xbuild-file=")) {
                String bf = arg.substring("-Xbuild-file=".length());
                Path p = Paths.get(bf);
                if (!p.isAbsolute()) p = base.resolve(p);
                result.add("-Xbuild-file=" + p.toString());
            } else if (arg.startsWith("-Xplugin=")) {
                String pl = arg.substring("-Xplugin=".length());
                Path p = Paths.get(pl);
                if (!p.isAbsolute()) p = base.resolve(p);
                result.add("-Xplugin=" + p.toString());
            } else if (arg.startsWith("-P") && i + 1 < args.size() && args.get(i + 1).startsWith("plugin:")) {
                result.add(arg);
                result.add(adjustPluginArg(args.get(i + 1), base));
                i++;
            } else if (arg.startsWith("-P") && arg.contains("plugin:")) {
                result.add(adjustPluginArg(arg, base));
            } else if (arg.startsWith("-")) {
                result.add(arg);
            } else if (isPathArgument(arg)) {
                Path p = Paths.get(arg);
                if (!p.isAbsolute()) {
                    p = base.resolve(p);
                }
                result.add(p.toString());
            } else {
                result.add(arg);
            }
        }
        return result;
    }

    private static String adjustPluginArg(String arg, Path base) {
        int eq = arg.indexOf('=');
        if (eq == -1) return arg;
        String key = arg.substring(0, eq);
        String val = arg.substring(eq + 1);
        if (key.endsWith(":sources") || key.endsWith(":classes") || key.endsWith(":stubs") || key.endsWith(":outputDir") || key.endsWith(":apclasspath")) {
            if (key.endsWith(":apclasspath")) {
                String[] parts = val.split(":");
                StringBuilder sb = new StringBuilder();
                for (int i = 0; i < parts.length; i++) {
                    if (i > 0) sb.append(":");
                    Path p = Paths.get(parts[i]);
                    if (!p.isAbsolute()) p = base.resolve(p);
                    sb.append(p.toString());
                }
                return key + "=" + sb.toString();
            } else {
                Path p = Paths.get(val);
                if (!p.isAbsolute()) p = base.resolve(p);
                return key + "=" + p.toString();
            }
        }
        return arg;
    }

    private static boolean isPathArgument(String s) {
        if (s == null || s.isEmpty()) return false;
        String lower = s.toLowerCase();
        return lower.endsWith(".kt")
            || lower.endsWith(".java")
            || lower.endsWith(".kts")
            || lower.endsWith(".jar")
            || lower.endsWith(".class")
            || lower.endsWith(".zip")
            || lower.endsWith(".dex")
            || lower.endsWith(".xml")
            || lower.endsWith(".rsp")
            || s.contains("/")
            || s.contains("\\");
    }

    private static final Object PROCESS_LOCK = new Object();

    private static String extractJsonField(String json, String field) {
        String pattern = "\"" + field + "\"";
        int idx = json.indexOf(pattern);
        if (idx == -1) return "";
        int colon = json.indexOf(':', idx + pattern.length());
        if (colon == -1) return "";
        int quoteStart = json.indexOf('\"', colon + 1);
        if (quoteStart == -1) return "";

        for (int i = quoteStart + 1; i < json.length(); i++) {
            char c = json.charAt(i);
            if (c == '\"' && json.charAt(i - 1) != '\\') {
                return unescapeJson(json.substring(quoteStart + 1, i));
            }
        }
        return "";
    }

    private static List<String> extractJsonArray(String json, String field) {
        List<String> list = new ArrayList<>();
        String pattern = "\"" + field + "\"";
        int idx = json.indexOf(pattern);
        if (idx == -1) return list;
        int arrStart = json.indexOf('[', idx + pattern.length());
        if (arrStart == -1) return list;

        int depth = 0;
        int arrEnd = -1;
        boolean inStr = false;
        for (int i = arrStart; i < json.length(); i++) {
            char c = json.charAt(i);
            if (c == '\"' && (i == 0 || json.charAt(i - 1) != '\\')) {
                inStr = !inStr;
            } else if (!inStr) {
                if (c == '[') depth++;
                else if (c == ']') {
                    depth--;
                    if (depth == 0) {
                        arrEnd = i;
                        break;
                    }
                }
            }
        }
        if (arrEnd == -1) return list;
        String content = json.substring(arrStart + 1, arrEnd).trim();
        if (content.isEmpty()) return list;

        boolean inQuote = false;
        StringBuilder current = new StringBuilder();
        for (int i = 0; i < content.length(); i++) {
            char c = content.charAt(i);
            if (c == '\"') {
                if (inQuote && i > 0 && content.charAt(i - 1) == '\\') {
                    current.append(c);
                } else {
                    inQuote = !inQuote;
                    if (!inQuote) {
                        list.add(unescapeJson(current.toString()));
                        current.setLength(0);
                    }
                }
            } else if (inQuote) {
                current.append(c);
            }
        }
        return list;
    }

    private static String escapeJson(String s) {
        if (s == null) return "";
        StringBuilder sb = new StringBuilder();
        for (int i = 0; i < s.length(); i++) {
            char c = s.charAt(i);
            switch (c) {
                case '\"': sb.append("\\\""); break;
                case '\\': sb.append("\\\\"); break;
                case '\b': sb.append("\\b"); break;
                case '\f': sb.append("\\f"); break;
                case '\n': sb.append("\\n"); break;
                case '\r': sb.append("\\r"); break;
                case '\t': sb.append("\\t"); break;
                default:
                    if (c < ' ') {
                        sb.append(String.format("\\u%04x", (int) c));
                    } else {
                        sb.append(c);
                    }
            }
        }
        return sb.toString();
    }

    private static String unescapeJson(String s) {
        return s.replace("\\\"", "\"").replace("\\\\", "\\").replace("\\n", "\n").replace("\\r", "\r").replace("\\t", "\t");
    }
}
