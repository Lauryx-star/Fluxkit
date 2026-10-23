import type { Collection, ProjectView } from './types';

export function folderPath(collection: Collection, id: string | null): string {
  const parts: string[] = [];
  for (let depth = 0; id && depth < 32; depth++) {
    const folder = collection.folders.find((folder) => folder.id === id);
    if (!folder) {
      parts.unshift('Missing folder');
      break;
    }
    parts.unshift(folder.name);
    id = folder.parentId;
  }
  return parts.join(' / ');
}
export function requestLocation(
  project: ProjectView | undefined,
  collectionId: string | null,
  folderId: string | null,
): string {
  const collection = project?.collections.find(
    (collection) => collection.data.id === collectionId,
  )?.data;
  if (!collection) {
    return 'Not in a collection';
  }
  return [collection.name, folderPath(collection, folderId)].filter(Boolean).join(' / ');
}
