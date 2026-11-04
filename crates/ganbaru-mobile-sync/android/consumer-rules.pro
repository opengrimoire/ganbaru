# The JNI exports use this exact class and these static method names.
-keep class org.opengrimoire.ganbaru.mobile.sync.SyncNative {
    public static native java.lang.String runPass(java.lang.String, java.lang.String);
    public static native void registerKeystore(org.opengrimoire.ganbaru.mobile.sync.SyncKeystore);
}
# Rust calls these key store methods by name and signature through JNI.
-keep class org.opengrimoire.ganbaru.mobile.sync.SyncKeystore {
    public byte[] wrap(java.lang.String, byte[], byte[]);
    public byte[] unwrap(java.lang.String, byte[], byte[]);
    public byte[] forget(java.lang.String);
}
