package net.fabricmc.loader.api;

import java.nio.file.Path;
import java.util.Optional;

/** Compile-only stub of the Fabric Loader API. The real class is provided by Fabric Loader at runtime. */
public interface FabricLoader {
    static FabricLoader getInstance() {
        throw new UnsupportedOperationException("compile-only stub");
    }

    Path getGameDir();

    Optional<ModContainer> getModContainer(String id);

    MappingResolver getMappingResolver();
}
