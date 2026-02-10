/**
 * Shared types for the plugin system.
 */

import type { Plugin } from "../sdk/types";

export interface ExternalPluginInfo {
  manifest: {
    id: string;
    name: string;
    version: string;
    description: string;
    author: string;
    main: string;
    permissions?: {
      tables?: {
        read?: string[];
        write?: string[];
        create?: string[];
      };
      // Direct format (alternative to tables.read/write)
      read?: string[];
      write?: string[];
      create?: string[];
      schemaName?: string;
    };
  };
  path: string;
}

export interface LoadedExternalPlugin {
  plugin: Plugin;
  discoveredManifest: ExternalPluginInfo["manifest"];
}
