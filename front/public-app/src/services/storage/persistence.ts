import { STORE_PREFIX } from './keys'

/**
 * Save data to localStorage
 * @param key - Storage key
 * @param data - Data to store (will be JSON.stringified)
 */
export function setStoreItem<T>(key: string, data: T): void {
    try {
      localStorage.setItem(STORE_PREFIX + key, JSON.stringify(data));
    } catch (error) {
      console.error(`Failed to save item ${key} to localStorage:`, data, error);
    }
}

/**
 * Retrieve data from localStorage with default value
 * @param key - Storage key
 * @param defaultValue - Default value to return if not found/invalid
 * @returns Parsed data or default value
 */
export function getStoreItem<T>(key: string, defaultValue: T | null = null): T | null {
    try {
      const item = localStorage.getItem(STORE_PREFIX + key);
      return item ? (JSON.parse(item) as T) : defaultValue;
    } catch (error) {
      console.error(`Failed to retrieve item ${key} from localStorage:`, error);
      localStorage.removeItem(STORE_PREFIX + key);
      return defaultValue;
    }
}

/**
 * Remove item from localStorage
 * @param key - Storage key to remove
 */
export function removeStoreItem(key: string): void {
    try {
      localStorage.removeItem(STORE_PREFIX + key);
    } catch (error) {
      console.error(`Failed to remove item ${key} from localStorage:`, error);
    }
}

/**
 * Clear all localStorage data
 */
export function clearStore(): void {
    try {
      localStorage.clear();
    } catch (error) {
      console.error('Failed to clear localStorage:', error);
    }
}

/**
 * Check if localStorage is available
 * @returns true if localStorage is available
 */
export function isStoreAvailable(): boolean {
    try {
      const testKey = '__storage_test__';
      localStorage.setItem(testKey, 'test');
      localStorage.removeItem(testKey);
      return true;
    } catch (e) {
      return false;
    }
}
