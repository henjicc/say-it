export interface DroppedFileRoute {
  id: string;
  accepts: (path: string) => boolean;
}

export const PLUGIN_PACKAGE_DROP_ROUTE: DroppedFileRoute = {
  id: "plugin-package",
  accepts: (path) => extensionOf(path) === "sayit",
};

export function extensionOf(path: string) {
  const dot = path.lastIndexOf(".");
  return dot >= 0 ? path.slice(dot + 1).toLowerCase() : "";
}

export function firstAcceptedPath(paths: string[], route: DroppedFileRoute) {
  return paths.find(route.accepts);
}
