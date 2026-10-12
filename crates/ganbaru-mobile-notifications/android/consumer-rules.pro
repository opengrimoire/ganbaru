# The private JNI export uses this exact class and static method name.
-keep class org.opengrimoire.ganbaruai.mobile.notifications.NativeFocusAuthority {
    public static native boolean isCurrent(long, long, long);
    public static native boolean isProcessCurrent(long);
}
