const database = () => new Promise<IDBDatabase>((resolve, reject) => {
  const request = indexedDB.open('hound-photos', 1);
  request.onupgradeneeded = () => request.result.createObjectStore('photos');
  request.onsuccess = () => resolve(request.result);
  request.onerror = () => reject(request.error);
});

export async function photoStore(key: string, file?: File): Promise<Blob | undefined> {
  const db = await database();
  return new Promise((resolve, reject) => {
    const transaction = db.transaction('photos', file ? 'readwrite' : 'readonly');
    const request = file ? transaction.objectStore('photos').put(file, key) : transaction.objectStore('photos').get(key);
    request.onsuccess = () => resolve(file ?? request.result);
    request.onerror = () => reject(request.error);
    transaction.oncomplete = () => db.close();
  });
}
