# Bind and close one native AF_UNIX socket without a mux, listener, or service.
# Windows PowerShell 5.1 lacks UnixDomainSocketEndPoint, so use Winsock directly.
# Binding leaves the socket's reparse-point leaf for the real cleanup guard.
Add-Type -TypeDefinition @'
using System;
using System.ComponentModel;
using System.Runtime.InteropServices;
using System.Text;

public static class WispProbeUnixSocket {
    [DllImport("ws2_32.dll")]
    static extern int WSAStartup(ushort version, IntPtr data);
    [DllImport("ws2_32.dll")]
    static extern int WSACleanup();
    [DllImport("ws2_32.dll")]
    static extern int WSAGetLastError();
    [DllImport("ws2_32.dll")]
    static extern UIntPtr socket(int family, int type, int protocol);
    [DllImport("ws2_32.dll")]
    static extern int bind(UIntPtr handle, byte[] address, int length);
    [DllImport("ws2_32.dll")]
    static extern int closesocket(UIntPtr handle);

    public static void Bind(string path) {
        byte[] name = Encoding.UTF8.GetBytes(path);
        if (name.Length > 107 || path.IndexOf('\0') >= 0)
            throw new ArgumentException("AF_UNIX path exceeds its byte budget");
        byte[] address = new byte[110]; // sockaddr_un: WORD family, char path[108].
        address[0] = 1; // AF_UNIX; Windows is little-endian.
        Array.Copy(name, 0, address, 2, name.Length);
        IntPtr data = Marshal.AllocHGlobal(512); // Fits WSADATA on x86 and x64.
        try {
            int error = WSAStartup(0x0202, data);
            if (error != 0) throw new Win32Exception(error);
            try {
                UIntPtr handle = socket(1, 1, 0); // AF_UNIX, SOCK_STREAM.
                UIntPtr invalid = UIntPtr.Size == 8
                    ? new UIntPtr(ulong.MaxValue) : new UIntPtr(uint.MaxValue);
                if (handle == invalid) throw new Win32Exception(WSAGetLastError());
                try {
                    if (bind(handle, address, address.Length) != 0)
                        throw new Win32Exception(WSAGetLastError());
                } finally {
                    if (closesocket(handle) != 0)
                        throw new Win32Exception(WSAGetLastError());
                }
            } finally {
                WSACleanup();
            }
        } finally {
            Marshal.FreeHGlobal(data);
        }
    }
}
'@
