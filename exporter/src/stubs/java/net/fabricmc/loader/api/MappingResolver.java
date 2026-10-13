package net.fabricmc.loader.api;

/** Compile-only stub of the Fabric Loader API. The real class is provided by Fabric Loader at runtime. */
public interface MappingResolver {
    String getCurrentRuntimeNamespace();

    String mapClassName(String namespace, String className);

    String mapFieldName(String namespace, String owner, String name, String descriptor);

    String mapMethodName(String namespace, String owner, String name, String descriptor);
}
