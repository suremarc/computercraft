package com.example;

import javax.net.ssl.*;
import java.io.ByteArrayInputStream;
import java.io.InputStream;
import java.nio.charset.StandardCharsets;
import java.security.KeyStore;
import java.security.Provider;
import java.security.Security;
import java.security.cert.Certificate;
import java.security.cert.CertificateException;
import java.security.cert.CertificateFactory;
import java.security.cert.X509Certificate;
import java.util.Arrays;
import java.util.Collection;

/**
 * Java agent that overlays an additional CA (e.g., Kubernetes cluster CA)
 * onto the JVM default trust store without -Djavax.net.ssl.trustStore.
 *
 * Usage:
 *   1) Replace CLUSTER_CA_PEM with the decoded PEM from kubeconfig
 *      (certificate-authority-data). If left as placeholder or invalid,
 *      the agent will log and fall back to the default trust store only.
 *   2) Build a jar with MANIFEST: Premain-Class: com.example.UnionTrustManagerAgent
 *   3) Launch: -javaagent:/path/union-tmf-agent.jar
 */
public final class UnionTrustManagerAgent {

    /** Paste your decoded PEM here. Multiple certs allowed (concatenate). */
    private static final String CLUSTER_CA_PEM = """
    -----BEGIN CERTIFICATE-----
    MIIDJzCCAg+gAwIBAgICBnUwDQYJKoZIhvcNAQELBQAwMzEVMBMGA1UEChMMRGln
    aXRhbE9jZWFuMRowGAYDVQQDExFrOHNhYXMgQ2x1c3RlciBDQTAeFw0yNTA5MTQx
    NDM5MDJaFw00NTA5MTQxNDM5MDJaMDMxFTATBgNVBAoTDERpZ2l0YWxPY2VhbjEa
    MBgGA1UEAxMRazhzYWFzIENsdXN0ZXIgQ0EwggEiMA0GCSqGSIb3DQEBAQUAA4IB
    DwAwggEKAoIBAQChlI3Oo+RY4YoEP5SVW7qKog6phFrAzbH6Md6GveMtlwF/o6Zf
    kcOkMQxMSpS7/wHjM43QHofRJzgEyq/4gKuj7cDDWLdWVqI6Q7IvASueqjNUw10m
    +VIxJgpGfo1xfoN9BGr3KEH36AfSpY1tCrhl1fxdFlbqyAzHYmagaIterHCjGumd
    FPk3Z3i7YnnlU7P01UuLb8WuWG4wk1GZKxDSsEsiUXqm7RjG3yhv/6Q9dhwA/uQI
    v/yVLhJUGdAFfaqfjHISh4RHZJ+xFV/e7Ng2SkFi59ZhFEoNRcb0Prpjw7yZRLAv
    1wMIiM+qv+nKw+x2HIDfabcy0Q4vD0/f7e+3AgMBAAGjRTBDMA4GA1UdDwEB/wQE
    AwIBhjASBgNVHRMBAf8ECDAGAQH/AgEAMB0GA1UdDgQWBBQJZOrDZJaxIfOrqo4v
    INHm6ckhxTANBgkqhkiG9w0BAQsFAAOCAQEAk16L+rb4VPdW4gVUZz0EqcLL7asu
    I9Lthwt2XHZCQbzdohHPYTtU9bE47ouSgXm8qpIPahx1leydy1LTjk2kRAt84REw
    kzKQJWlEPcO/aS7AzSJSXmIIXajys5PgPuEm5Q31P3rWMk1mWjwj0Fuq+0GnFrwg
    V7JCGXyVUn6w9C6/5volS2kc/041EicRaTrXhViZE4k3a/ml4/7Qh0wG5kYPtpCA
    dtHbQlBpLYjTg+GiVKTp2Em1vNnw/ACJu0f+cfIbzBpi407uui+UbK5G/a2zG8Zn
    KjlxsKIVCiqevoCuuDjW2+6Q04A7gsSpNWQM+6NMbg91ICp5yOpFZnicDA==
    -----END CERTIFICATE-----
    """;

    /** Original default TMF algorithm (captured before override). */
    private static String ORIGINAL_ALG = null;

    public static void premain(String agentArgs) {
        try {
            ORIGINAL_ALG = Security.getProperty("ssl.TrustManagerFactory.algorithm");
            Security.addProvider(new ExtraProvider(CLUSTER_CA_PEM));
            Security.setProperty("ssl.TrustManagerFactory.algorithm", "UnionX509");
            info("Installed UnionX509 TrustManagerFactory (default + extra CA). Original default: " + ORIGINAL_ALG);
        } catch (Throwable t) {
            // Fail-safe: never block JVM startup
            err("Failed to install UnionX509 provider, continuing with platform defaults", t);
        }
    }

    // at top-level in UnionTrustManagerAgent
    private enum L { OFF, ERROR, WARN, INFO, DEBUG }
    private static L LOG_LVL = L.WARN; // default

    private static void initLogLevel() {
        String v = System.getProperty("extra.ca.agent.log", "info").toUpperCase();
        try { LOG_LVL = L.valueOf(v); } catch (Exception ignore) { LOG_LVL = L.WARN; }
    }

    private static void log(L lvl, String msg, Throwable t) {
        if (lvl.ordinal() <= LOG_LVL.ordinal()) {
            String p = "[ExtraCaAgent] " + (lvl==L.ERROR?"ERROR: ":lvl==L.WARN?"WARN: ":"");
            System.out.println(p + msg + (t!=null? " ("+t+")" : ""));
        }
    }
    private static void log(L lvl, String msg) { log(lvl, msg, null); }

    /** Simple stdout log helpers (some hosts prefix with [Info]/[ERROR] anyway). */
    private static void info(String msg) { log(L.INFO, msg); }
    private static void debug(String msg) { log(L.DEBUG, msg); }
    private static void err(String msg, Throwable t) { log(L.ERROR, msg, t); }

    /** Provider exposing TrustManagerFactory.UnionX509 -> ExtraTmfSpi. */
    public static final class ExtraProvider extends Provider {
        public ExtraProvider(String pem) {
            super("ExtraProvider", 1.0, "Union trust manager provider");
            put("TrustManagerFactory.UnionX509", ExtraTmfSpi.class.getName());
            ExtraTmfSpi.EXTRA_PEM = pem; // pass PEM via static
        }
    }

    /** TMF SPI which returns a union(X509TM(default), X509TM(extra)) with fail-open behavior. */
    public static final class ExtraTmfSpi extends TrustManagerFactorySpi {
        static String EXTRA_PEM; // injected by provider
        private X509TrustManager union;

        @Override
        protected void engineInit(KeyStore ks) {
            this.union = buildUnionSafely(ks);
        }

        @Override
        protected void engineInit(ManagerFactoryParameters spec) {
            // Most callers pass null/none; ignore params and build from system defaults
            this.union = buildUnionSafely(null);
        }

        @Override
        protected TrustManager[] engineGetTrustManagers() {
            return new TrustManager[]{ union };
        }

        private static X509TrustManager buildUnionSafely(KeyStore ks) {
            try {
                X509TrustManager def = defaultTmFrom(ks); // may throw
                X509TrustManager extra = tryExtraTm(EXTRA_PEM); // returns null on failure/placeholder
                if (extra == null) {
                    info("Extra CA not loaded (placeholder or invalid PEM) — using default trust store only");
                    return def;
                }
                info("Extra CA loaded — enabling union trust (default + extra)");
                return unionOf(def, extra);
            } catch (Throwable t) {
                // Absolutely do not break JVM defaults — fall back to default only
                err("UnionX509 init failed, falling back to default trust store", t);
                try {
                    return defaultTmFrom(ks);
                } catch (Throwable t2) {
                    // If even default fails, rethrow — nothing more we can do
                    throw new RuntimeException("No default X509TrustManager available", t2);
                }
            }
        }

        /** Build platform default X509TrustManager using a concrete algorithm (avoid recursion). */
        private static X509TrustManager defaultTmFrom(KeyStore ks) throws Exception {
            TrustManagerFactory defTmf = getBuiltinTmf();
            defTmf.init(ks); // ks==null => system default cacerts
            return firstX509(defTmf.getTrustManagers());
        }

        /** Try to build X509TrustManager from embedded PEM. Returns null if PEM missing/invalid. */
        private static X509TrustManager tryExtraTm(String pem) {
            try {
                if (pem == null) return null;
                String trimmed = pem.trim();
                if (trimmed.isEmpty() || !trimmed.contains("BEGIN CERTIFICATE") ||
                    trimmed.contains("<PASTE YOUR PEM")) {
                    return null; // placeholder or empty
                }
                KeyStore extraKs = KeyStore.getInstance(KeyStore.getDefaultType());
                extraKs.load(null, null);
                CertificateFactory cf = CertificateFactory.getInstance("X.509");
                try (InputStream in = new ByteArrayInputStream(trimmed.getBytes(StandardCharsets.US_ASCII))) {
                    Collection<? extends Certificate> certs = cf.generateCertificates(in);
                    int i = 0;
                    for (Certificate c : certs) extraKs.setCertificateEntry("extra-" + (i++), c);
                }
                TrustManagerFactory tmf = getBuiltinTmf();
                tmf.init(extraKs);
                return firstX509(tmf.getTrustManagers());
            } catch (Throwable t) {
                err("Failed to load extra CA PEM, ignoring", t);
                return null;
            }
        }

        /** Choose a concrete, built-in TMF algorithm (PKIX preferred; fallback SunX509). */
        private static TrustManagerFactory getBuiltinTmf() throws Exception {
            if (ORIGINAL_ALG != null && !"UnionX509".equals(ORIGINAL_ALG)) {
                try { return TrustManagerFactory.getInstance(ORIGINAL_ALG); } catch (Exception ignore) {}
            }
            try { return TrustManagerFactory.getInstance("PKIX"); }
            catch (Exception ignore) { return TrustManagerFactory.getInstance("SunX509"); }
        }

        private static X509TrustManager firstX509(TrustManager[] tms) {
            for (TrustManager tm : tms) if (tm instanceof X509TrustManager) return (X509TrustManager) tm;
            throw new IllegalStateException("No X509TrustManager available");
        }

        /** A trust manager that accepts if either delegate accepts; logs on server checks. */
        private static X509TrustManager unionOf(X509TrustManager def, X509TrustManager extra) {
            return new X509TrustManager() {
                @Override
                public void checkClientTrusted(X509Certificate[] chain, String authType) throws CertificateException {
                    try { def.checkClientTrusted(chain, authType); }
                    catch (CertificateException e) { extra.checkClientTrusted(chain, authType); }
                }
                @Override
                public void checkServerTrusted(X509Certificate[] chain, String authType) throws CertificateException {
                    debug("HTTPS handshake via UnionX509; authType=" + authType);
                    for (X509Certificate cert : chain) {
                        debug("  Subject=" + cert.getSubjectDN() + "  Issuer=" + cert.getIssuerDN());
                    }
                    try {
                        def.checkServerTrusted(chain, authType);
                        debug("  -> accepted by default trust store");
                    } catch (CertificateException e) {
                        debug("  -> rejected by default; trying extra CA");
                        extra.checkServerTrusted(chain, authType);
                        debug("  -> accepted by extra CA");
                    }
                }
                @Override
                public X509Certificate[] getAcceptedIssuers() {
                    X509Certificate[] a = def.getAcceptedIssuers(), b = extra.getAcceptedIssuers();
                    X509Certificate[] m = Arrays.copyOf(a, a.length + b.length);
                    System.arraycopy(b, 0, m, a.length, b.length);
                    return m;
                }
            };
        }
    }
}
