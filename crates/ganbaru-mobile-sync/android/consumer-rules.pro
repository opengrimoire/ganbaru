# The JNI export uses this exact class and static method name.
-keep class org.opengrimoire.ganbaru.mobile.sync.SyncNative {
    public static native java.lang.String runPass(java.lang.String, java.lang.String);
}
