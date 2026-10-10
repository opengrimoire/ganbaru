import type { collections as enCollections } from "../en/collections";
import type { MessageShape } from "../types";

export const collections = {
  property: {
    add: "Agregar propiedad",
    name: "Nombre de la propiedad",
    namePlaceholder: "Escribe el nombre de la propiedad...",
    selectType: "Selecciona un tipo",
    searchTypes: "Buscar tipos",
    noTypes: "No hay tipos que coincidan",
    nameExists: "Ya existe una propiedad con ese nombre.",
    renameFailed: (message: string) => `No se pudo renombrar la propiedad: ${message}`,
    icon: "Icono de la propiedad",
    iconFailed: (message: string) => `No se pudo cambiar el icono de la propiedad: ${message}`,
    editProperty: "Editar propiedad",
    filter: "Filtrar",
    sortAscending: "Orden ascendente",
    sortDescending: "Orden descendente",
    calculate: "Calcular",
    freeze: "Fijar hasta esta columna",
    unfreeze: "Liberar columnas",
    hide: "Ocultar",
    wrap: "Ajustar contenido",
    moveLeft: "Mover a la izquierda",
    moveRight: "Mover a la derecha",
    insertLeft: "Insertar a la izquierda",
    insertRight: "Insertar a la derecha",
    duplicate: "Duplicar propiedad (vacía)",
  },
} as const satisfies MessageShape<typeof enCollections>;
