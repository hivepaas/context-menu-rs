/* Loads a Nautilus extension as Nautilus does, and asks it what it shows.
 *
 *   harness <libnautilus-extension.so.4> <extension.so> <files>...
 *
 * Each <files> is a choice of files, comma-separated: "a.zip", "photos/" (a
 * folder), "sftp:b.zip" (a remote file); for each, a line says what the
 * extension shows: "a.zip: Title > First, Second". With $OUT set, the first
 * item shown is then chosen, and the harness waits for $OUT to appear.
 *
 * libnautilus-extension's headers need not be installed: what this uses of
 * it is looked up at run time, and the one struct it fills is copied here
 * from nautilus-file-info.h. */

#include <dlfcn.h>
#include <gio/gio.h>
#include <glib-object.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static GType (*file_info_get_type) (void);
static GList *(*get_file_items) (GObject *provider, GList *files);
static GList *(*menu_get_items) (GObject *menu);
static void (*menu_item_activate) (GObject *item);

/* NautilusFileInfoInterface, from nautilus-file-info.h. */
typedef struct
{
    GTypeInterface g_iface;
    gboolean (*is_gone) (gpointer);
    char *(*get_name) (gpointer);
    char *(*get_uri) (gpointer);
    char *(*get_parent_uri) (gpointer);
    char *(*get_uri_scheme) (gpointer);
    char *(*get_mime_type) (gpointer);
    gboolean (*is_mime_type) (gpointer, const char *);
    gboolean (*is_directory) (gpointer);
    void (*add_emblem) (gpointer, const char *);
    char *(*get_string_attribute) (gpointer, const char *);
    void (*add_string_attribute) (gpointer, const char *, const char *);
    void (*invalidate_extension_info) (gpointer);
    char *(*get_activation_uri) (gpointer);
    GFileType (*get_file_type) (gpointer);
    GFile *(*get_location) (gpointer);
    GFile *(*get_parent_location) (gpointer);
    gpointer (*get_parent_info) (gpointer);
    GMount *(*get_mount) (gpointer);
    gboolean (*can_write) (gpointer);
} FileInfoIface;

/* A file as a choice names it, in /home/u. */
typedef struct
{
    GObject parent;
    char *name;
    char *scheme;
    gboolean dir;
} TestFile;

typedef struct
{
    GObjectClass parent;
} TestFileClass;

static char *
file_name (gpointer f)
{
    return g_strdup (((TestFile *) f)->name);
}

static char *
file_scheme (gpointer f)
{
    return g_strdup (((TestFile *) f)->scheme);
}

static gboolean
file_is_dir (gpointer f)
{
    return ((TestFile *) f)->dir;
}

static GFile *
file_location (gpointer f)
{
    char *path = g_build_filename ("/home/u", ((TestFile *) f)->name, NULL);
    GFile *file = g_file_new_for_path (path);
    g_free (path);
    return file;
}

static void
file_iface_init (gpointer g_iface, gpointer data)
{
    FileInfoIface *iface = g_iface;
    iface->get_name = file_name;
    iface->get_uri_scheme = file_scheme;
    iface->is_directory = file_is_dir;
    iface->get_location = file_location;
}

static GType
test_file_type (void)
{
    static GType type = 0;
    if (type == 0)
    {
        GTypeInfo info = { sizeof (TestFileClass), NULL, NULL, NULL, NULL, NULL,
                           sizeof (TestFile), 0, NULL, NULL };
        GInterfaceInfo iface = { file_iface_init, NULL, NULL };
        type = g_type_register_static (G_TYPE_OBJECT, "HarnessFile", &info, 0);
        g_type_add_interface_static (type, file_info_get_type (), &iface);
    }
    return type;
}

/* A GTypeModule, which is abstract, that loads nothing itself. */
typedef struct
{
    GTypeModule parent;
} TestModule;

typedef struct
{
    GTypeModuleClass parent;
} TestModuleClass;

static gboolean
module_load (GTypeModule *module)
{
    return TRUE;
}

static void
module_unload (GTypeModule *module)
{
}

static void
module_class_init (gpointer klass, gpointer data)
{
    G_TYPE_MODULE_CLASS (klass)->load = module_load;
    G_TYPE_MODULE_CLASS (klass)->unload = module_unload;
}

static GType
test_module_type (void)
{
    GTypeInfo info = { sizeof (TestModuleClass), NULL, NULL, module_class_init, NULL, NULL,
                       sizeof (TestModule), 0, NULL, NULL };
    return g_type_register_static (G_TYPE_TYPE_MODULE, "HarnessModule", &info, 0);
}

static GList *
files_of (const char *choice)
{
    GList *files = NULL;
    char **names = g_strsplit (choice, ",", -1);
    for (char **n = names; *n != NULL; n++)
    {
        TestFile *f = g_object_new (test_file_type (), NULL);
        const char *name = *n;
        f->scheme = g_strdup (g_str_has_prefix (name, "sftp:") ? "sftp" : "file");
        if (g_str_has_prefix (name, "sftp:"))
        {
            name += strlen ("sftp:");
        }
        f->dir = g_str_has_suffix (name, "/");
        f->name = g_strndup (name, strlen (name) - (f->dir ? 1 : 0));
        files = g_list_append (files, f);
    }
    g_strfreev (names);
    return files;
}

static void *
need (void *lib, const char *symbol)
{
    void *found = dlsym (lib, symbol);
    if (found == NULL)
    {
        fprintf (stderr, "harness: no %s\n", symbol);
        exit (2);
    }
    return found;
}

int
main (int argc, char **argv)
{
    if (argc < 3)
    {
        fprintf (stderr, "usage: harness <libnautilus-extension> <extension> <files>...\n");
        return 2;
    }
    /* Nautilus has its library loaded, globally, before any extension. */
    void *nautilus = dlopen (argv[1], RTLD_NOW | RTLD_GLOBAL);
    if (nautilus == NULL)
    {
        fprintf (stderr, "harness: %s\n", dlerror ());
        return 2;
    }
    file_info_get_type = need (nautilus, "nautilus_file_info_get_type");
    get_file_items = need (nautilus, "nautilus_menu_provider_get_file_items");
    menu_get_items = need (nautilus, "nautilus_menu_get_items");
    menu_item_activate = need (nautilus, "nautilus_menu_item_activate");

    /* As nautilus-module.c loads one. */
    void *extension = dlopen (argv[2], RTLD_NOW | RTLD_LOCAL);
    if (extension == NULL)
    {
        fprintf (stderr, "harness: %s\n", dlerror ());
        return 2;
    }
    void (*initialize) (GTypeModule *) = need (extension, "nautilus_module_initialize");
    void (*list_types) (const GType **, int *) = need (extension, "nautilus_module_list_types");
    GTypeModule *module = g_object_new (test_module_type (), NULL);
    g_type_module_use (module);
    initialize (module);
    const GType *types = NULL;
    int n = 0;
    list_types (&types, &n);
    printf ("types: %d\n", n);
    if (n == 0)
    {
        return 0;
    }
    GObject *provider = g_object_new (types[0], NULL);

    GObject *first = NULL;
    for (int i = 3; i < argc; i++)
    {
        GList *files = files_of (argv[i]);
        GList *items = get_file_items (provider, files);
        printf ("%s:", argv[i]);
        for (GList *l = items; l != NULL; l = l->next)
        {
            char *label = NULL;
            GObject *menu = NULL;
            g_object_get (l->data, "label", &label, "menu", &menu, NULL);
            printf ("%s %s", l == items ? "" : ",", label);
            if (menu != NULL)
            {
                GList *sub = menu_get_items (menu);
                printf (" >");
                for (GList *s = sub; s != NULL; s = s->next)
                {
                    char *sub_label = NULL;
                    g_object_get (s->data, "label", &sub_label, NULL);
                    printf ("%s %s", s == sub ? "" : ",", sub_label);
                    if (first == NULL)
                    {
                        first = g_object_ref (s->data);
                    }
                    g_free (sub_label);
                }
                g_list_free_full (sub, g_object_unref);
                g_object_unref (menu);
            }
            else if (first == NULL)
            {
                first = g_object_ref (l->data);
            }
            g_free (label);
        }
        printf ("\n");
        g_list_free_full (items, g_object_unref);
        g_list_free_full (files, g_object_unref);
    }
    fflush (stdout);

    const char *out = g_getenv ("OUT");
    if (first != NULL && out != NULL)
    {
        menu_item_activate (first);
        for (int i = 0; i < 100 && !g_file_test (out, G_FILE_TEST_EXISTS); i++)
        {
            g_usleep (50000);
        }
        g_object_unref (first);
    }
    return 0;
}
