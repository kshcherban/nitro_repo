import { apiURL } from "@/config";

export interface RepositoryRouteTarget {
  storage_name: string;
  name: string;
}

export function createRepositoryRoute(
  repository: RepositoryRouteTarget,
  route?: string,
): string {
  let backend = apiURL;
  if (backend.endsWith("/")) {
    backend = backend.substring(0, backend.length - 1);
  }
  if (route === undefined) {
    return `${backend}/repositories/${repository.storage_name}/${repository.name}`;
  }
  return `${backend}/repositories/${repository.storage_name}/${repository.name}/${route}`;
}
