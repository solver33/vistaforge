package org.slf4j;

/** Compile-only stub of SLF4J. Minecraft ships the real library. */
public interface Logger {
    void info(String msg);

    void info(String format, Object... arguments);

    void warn(String msg);

    void warn(String msg, Throwable t);

    void error(String msg, Throwable t);
}
