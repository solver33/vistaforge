package org.slf4j;

/** Compile-only stub of SLF4J. Minecraft ships the real library. */
public final class LoggerFactory {
    private LoggerFactory() {
    }

    public static Logger getLogger(String name) {
        throw new UnsupportedOperationException("compile-only stub");
    }
}
