mod adapter;
mod command;
mod logger;

use std::{ffi::c_void, ptr::null_mut, sync::Arc, thread};

use adapter::AndroidStreamReceiverAdapter;
use command::{catcher, copy_from_byte_array, JVM};
use jni::{
    objects::{JByteArray, JClass, JObject, JString},
    sys::JNI_VERSION_1_6,
    JNIEnv, JavaVM,
};

use jni_macro::jni_exports;
use logger::AndroidLogger;
use transport::{
    adapter::{StreamReceiverAdapter, StreamReceiverAdapterExt, StreamSenderAdapter},
    Transport, TransportOptions,
};

/// JNI_OnLoad
///
/// jint JNI_OnLoad(JavaVM *vm, void *reserved);
///
/// The VM calls JNI_OnLoad when the native library is loaded (for example,
/// through System.loadLibrary). JNI_OnLoad must return the JNI version needed
/// by the native library.
/// In order to use any of the new JNI functions, a native library must export a
/// JNI_OnLoad function that returns JNI_VERSION_1_2. If the native library does
/// not export a JNI_OnLoad function, the VM assumes that the library only
/// requires JNI version JNI_VERSION_1_1. If the VM does not recognize the
/// version number returned by JNI_OnLoad, the VM will unload the library and
/// act as if the library was +never loaded.
///
/// JNI_Onload_L(JavaVM *vm, void *reserved);
///
/// If a library L is statically linked, then upon the first invocation of
/// System.loadLibrary("L") or equivalent API, a JNI_OnLoad_L function will be
/// invoked with the same arguments and expected return value as specified for
/// the JNI_OnLoad function. JNI_OnLoad_L must return the JNI version needed by
/// the native library. This version must be JNI_VERSION_1_8 or later. If the
/// VM does not recognize the version number returned by JNI_OnLoad_L, the VM
/// will act as if the library was never loaded.
///
/// LINKAGE:
/// Exported from native libraries that contain native method implementation.
#[no_mangle]
pub extern "system" fn JNI_OnLoad(vm: JavaVM, _: *mut c_void) -> i32 {
    AndroidLogger::init();
    transport::startup();
    JVM.lock().unwrap().replace(vm);

    JNI_VERSION_1_6
}

/// JNI_OnUnload
///
/// void JNI_OnUnload(JavaVM *vm, void *reserved);
///
/// The VM calls JNI_OnUnload when the class loader containing the native
/// library is garbage collected. This function can be used to perform cleanup
/// operations. Because this function is called in an unknown context (such as
/// from a finalizer), the programmer should be conservative on using Java VM
/// services, and refrain from arbitrary Java call-backs.
/// Note that JNI_OnLoad and JNI_OnUnload are two functions optionally supplied
/// by JNI libraries, not exported from the VM.
///
/// JNI_OnUnload_L(JavaVM *vm, void *reserved);
///
/// When the class loader containing a statically linked native library L is
/// garbage collected, the VM will invoke the JNI_OnUnload_L function of the
/// library if such a function is exported. This function can be used to perform
/// cleanup operations. Because this function is called in an unknown context
/// (such as from a finalizer), the programmer should be conservative on using
/// Java VM services, and refrain from arbitrary Java call-backs.
///
/// Informational Note:
/// The act of loading a native library is the complete process of making the
/// library and its native entry points known and registered to the Java VM and
/// runtime. Note that simply performing operating system level operations to
/// load a native library, such as dlopen on a UNIX(R) system, does not fully
/// accomplish this goal. A native function is normally called from the Java
/// class loader to perform a call to the host operating system that will load
/// the library into memory and return a handle to the native library. This
/// handle will be stored and used in subsequent searches for native library
/// entry points. The Java native class loader will complete the load process
/// once the handle is successfully returned to register the library.
///
/// LINKAGE:
/// Exported from native libraries that contain native method implementation.
#[no_mangle]
pub extern "system" fn JNI_OnUnload(_: JavaVM, _: *mut c_void) {
    transport::shutdown();
}

mod objects {
    use anyhow::{anyhow, Ok};
    use jni::{
        objects::{JObject, JValueGen},
        JNIEnv,
    };

    use transport::adapter::{StreamBufferInfo, StreamKind};

    /// /**
    ///  * Streaming data information.
    ///  */
    /// data class StreamBufferInfo(val kind: Int) {
    ///     var flags: Int = 0;
    /// }
    pub fn to_stream_buffer_info(
        env: &mut JNIEnv,
        info: &JObject,
    ) -> anyhow::Result<StreamBufferInfo> {
        let kind = if let JValueGen::Int(kind) = env.get_field(info, "kind", "I")? {
            kind
        } else {
            return Err(anyhow!("kind not a int."));
        };

        let flags = if let JValueGen::Int(flags) = env.get_field(info, "flags", "I")? {
            flags
        } else {
            return Err(anyhow!("flags not a int."));
        };

        let timestamp = if let JValueGen::Long(timestamp) = env.get_field(info, "timestamp", "J")? {
            timestamp as u64
        } else {
            return Err(anyhow!("timestamp not a long."));
        };

        Ok(
            match StreamKind::try_from(kind as u8).map_err(|_| anyhow!("kind unreachable"))? {
                StreamKind::Video => StreamBufferInfo::Video(flags, timestamp),
                StreamKind::Audio => StreamBufferInfo::Audio(flags, timestamp),
            },
        )
    }
}

/// package mirror.java
///
/// /**
///  * Data Stream Receiver Adapter
///  *
///  * Used to receive data streams from the network.
///  */
/// abstract class ReceiverAdapter {
///     /**
///      * Triggered when data arrives in the network.
///      *
///      * Note: If the buffer is empty, the current network connection has been
///      * closed or suddenly interrupted.
///      */
///     abstract fun sink(kind: Int, buf: ByteArray)
///     abstract fun close()
/// }
///
/// /**
///  * Data Stream Receiver Adapter Factory
///  */
/// abstract class ReceiverAdapterFactory {
///     /**
///      * Called when a new connection comes in.
///      *
///      * You can choose to return Null, which will cause the connection to be
///        rejected.
///      */
///     abstract fun connect(id: Int, ip: String, description: ByteArray):
/// ReceiverAdapter? }
///
/// data class BufferInfo(
///
/// )
///
/// /**
///  * Data Stream Sender Adapter
///  */
/// class SenderAdapter constructor(
///     private val sender: (ByteArray) -> Unit,
///     private val releaser: () -> Unit
/// ) {
///     /**
///      * Sends packets into the network.
///      *
///      * If an empty packet is sent, the remote connection will be closed.
///      */
///     fun send(buf: ByteArray) {
///         sender(buf)
///     }
///
///     /**
///      * Release this sender.
///      */
///     fun release() {
///         releaser()
///     }
/// }
///
/// /**
///  * class of projection screen.
///  *
///  * Encapsulates sending data and receiving data and provides mechanisms for
///  * auto-discovery and auto-join.
///  */
/// class Mirror constructor(
///     private val bind: String,
///     private val adapterFactory: ReceiverAdapterFactory
/// ) {
///     private var mirror: Long = 0
///
///     init {
///         mirror = createMirror(bind,
/// createStreamReceiverAdapterFactory(adapterFactory))         if (mirror ==
/// 0L) {             throw Exception("failed to create mirror!")
///         }
///     }
///
///     /**
///      * To create a sender, you can specify the sender's group ID so that
///        others
///      * can decide whether to receive your data based on the group ID.
///      */
///     fun createSender(id: Int, description: ByteArray): SenderAdapter {
///         val adapter = createStreamSenderAdapter()
///         if (adapter == 0L) {
///             throw Exception("failed to create sender adapter!")
///         }
///
///         if (!createSender(mirror, id, description, adapter)) {
///             throw Exception("failed to create mirror sender adapter!")
///         }
///
///         return SenderAdapter(
///             { buf -> sendBufToSender(adapter, buf) },
///             { -> releaseSenderAdapter(adapter) },
///         )
///     }
///
///     /**
///      * Release this instance.
///      */
///     fun release() {
///         if (mirror != 0L) {
///             releaseMirror(mirror)
///         }
///     }
///
///     companion object {
///         init {
///             System.loadLibrary("mirror_exports")
///         }
///     }
///
///     /**
///      * Create a stream receiver adapter factory where the return value is a
///      * pointer to the instance, and you need to check that the returned
///        pointer
///      * is not Null.
///      */
///     private external fun createStreamReceiverAdapterFactory(adapterFactory:
/// ReceiverAdapterFactory): Long
///
///     /**
///      * Creates a mirror instance, the return value is a pointer, and you
///        need to
///      * check that the pointer is valid.
///      */
///     private external fun createMirror(
///         bind: String,
///         adapterFactory: Long
///     ): Long
///
///     /**
///      * Free the mirror instance pointer.
///      */
///     private external fun releaseMirror(mirror: Long)
///
///     /**
///      * Creates an instance of the stream sender adapter, the return value is
///        a
///      * pointer and you need to check if the pointer is valid.
///      */
///     private external fun createStreamSenderAdapter(kind: Int): Long
///
///     /**
///      * Release the stream sender adapter.
///      */
///     private external fun releaseStreamSenderAdapter(adapter: Long)
///
///     /**
///      * Creates the sender, the return value indicates whether the creation
///        was
///      * successful or not.
///      */
///     private external fun createSender(
///         mirror: Long,
///         id: Int,
///         description: ByteArray,
///         adapter: Long
///     ): Boolean
///
///     /**
///      * Sends the packet to the sender instance.
///      */
///     private external fun sendBufToSender(
///         adapter: Long,
///         buf: ByteArray,
///         info: BufferInfo
///     )
/// }
struct Mirror;

#[jni_exports(package = "com.github.mycrl.mirror")]
impl Mirror {
    /// /**
    ///  * Create a stream receiver adapter where the return value is a
    ///  * pointer to the instance, and you need to check that the returned
    ///  * pointer is not Null.
    ///  */
    /// private external fun createStreamReceiverAdapter(adapter:
    /// ReceiverAdapter): Long
    pub fn create_stream_receiver_adapter(
        mut env: JNIEnv,
        _this: JClass,
        callback: JObject,
    ) -> *const Arc<StreamReceiverAdapter> {
        catcher(&mut env, |env| {
            let adapter = AndroidStreamReceiverAdapter {
                callback: env.new_global_ref(callback)?,
            };

            let stream_adapter = StreamReceiverAdapter::new();
            let stream_adapter_ = Arc::downgrade(&stream_adapter);
            thread::Builder::new()
                .name("MirrorJniStreamReceiverThread".to_string())
                .spawn(move || {
                    while let Some(stream_adapter) = stream_adapter_.upgrade() {
                        if let Some((buf, kind, flags, timestamp)) = stream_adapter.next() {
                            if !adapter.sink(buf, kind, flags, timestamp) {
                                break;
                            }
                        } else {
                            break;
                        }
                    }

                    log::info!("StreamReceiverAdapter is closed");

                    adapter.close();
                })?;

            Ok(Box::into_raw(Box::new(stream_adapter)))
        })
        .unwrap_or_else(null_mut)
    }

    /// /**
    ///  * Free the stream receiver adapter instance pointer.
    ///  */
    /// private external fun releaseStreamReceiverAdapter(adapter: Long)
    pub fn release_stream_receiver_adapter(
        _env: JNIEnv,
        _this: JClass,
        ptr: *const Arc<StreamReceiverAdapter>,
    ) {
        unsafe { Box::from_raw(ptr as *mut Arc<StreamReceiverAdapter>) }.close();
    }

    /// /**
    ///  * Creates a mirror instance, the return value is a pointer, and you
    ///    need to
    ///  * check that the pointer is valid.
    ///  */
    /// private external fun createMirror(
    ///     bind: String,
    ///     adapterFactory: Long
    /// ): Long
    pub fn create_mirror(
        mut env: JNIEnv,
        _this: JClass,
        server: JString,
        multicast: JString,
        mtu: i32,
    ) -> *const Transport {
        catcher(&mut env, |env| {
            let server: String = env.get_string(&server)?.into();
            let multicast: String = env.get_string(&multicast)?.into();

            Ok(Box::into_raw(Box::new(Transport::new(TransportOptions {
                server: server.parse()?,
                multicast: multicast.parse()?,
                mtu: mtu as usize,
            })?)))
        })
        .unwrap_or_else(null_mut)
    }

    /// /**
    ///  * Free the mirror instance pointer.
    ///  */
    /// private external fun releaseMirror(mirror: Long)
    pub fn release_mirror(_env: JNIEnv, _this: JClass, ptr: *const Transport) {
        drop(unsafe { Box::from_raw(ptr as *mut Transport) })
    }

    /// /**
    ///  * Creates an instance of the stream sender adapter, the return value is
    ///    a
    ///  * pointer and you need to check if the pointer is valid.
    ///  */
    /// private external fun createStreamSenderAdapter(kind: Int): Long
    pub fn create_stream_sender_adapter(
        _env: JNIEnv,
        _this: JClass,
    ) -> *const Arc<StreamSenderAdapter> {
        Box::into_raw(Box::new(StreamSenderAdapter::new(false)))
    }

    /// /**
    ///  * Get whether the sender uses multicast transmission
    ///  */
    /// private external fun senderGetMulticast(adapter: Long): Boolean
    pub fn sender_get_multicast(
        _env: JNIEnv,
        _this: JClass,
        ptr: *const Arc<StreamSenderAdapter>,
    ) -> i32 {
        unsafe { &*ptr }.get_multicast() as i32
    }

    /// /**
    ///  * Set whether the sender uses multicast transmission
    ///  */
    /// private external fun senderSetMulticast(adapter: Long, is_multicast:
    /// Boolean)
    pub fn sender_set_multicast(
        _env: JNIEnv,
        _this: JClass,
        ptr: *const Arc<StreamSenderAdapter>,
        is_multicast: i32,
    ) {
        unsafe { &*ptr }.set_multicast(is_multicast != 0)
    }

    /// /**
    ///  * Release the stream sender adapter.
    ///  */
    /// private external fun releaseStreamSenderAdapter(adapter: Long)
    pub fn release_stream_sender_adapter(
        _env: JNIEnv,
        _this: JClass,
        ptr: *const Arc<StreamSenderAdapter>,
    ) {
        unsafe { Box::from_raw(ptr as *mut Arc<StreamSenderAdapter>) }.close();
    }

    /// /**
    ///  * Creates the sender, the return value indicates whether the creation
    ///  * was successful or not.
    ///  */
    /// private external fun createSender(
    ///     mirror: Long,
    ///     id: Int,
    ///     description: ByteArray,
    ///     adapter: Long
    /// ): Boolean
    pub fn create_sender(
        mut env: JNIEnv,
        _this: JClass,
        mirror: *const Transport,
        id: i32,
        adapter: *const Arc<StreamSenderAdapter>,
    ) -> i32 {
        catcher(&mut env, |_| {
            unsafe { &*mirror }.create_sender(id as u32, unsafe { &*adapter })?;
            Ok(true)
        })
        .unwrap_or(false) as i32
    }

    /// /**
    ///  * Sends the packet to the sender instance.
    ///  */
    /// private external fun sendBufToSender(
    ///     adapter: Long,
    ///     buf: ByteArray,
    ///     info: BufferInfo
    /// )
    pub fn send_buf_to_sender(
        mut env: JNIEnv,
        _this: JClass,
        adapter: *const Arc<StreamSenderAdapter>,
        info: JObject,
        buf: JByteArray,
    ) {
        catcher(&mut env, |env| {
            let buf = copy_from_byte_array(env, &buf)?;
            let info = objects::to_stream_buffer_info(env, &info)?;
            unsafe { &*adapter }.send(buf, info);

            Ok(())
        });
    }

    /// /**
    ///  * Creates the receiver, the return value indicates whether the creation
    ///  * was successful or not.
    ///  */
    /// private external fun createReceiver(
    ///     mirror: Long,
    ///     addr: String,
    ///     adapter: Long
    /// ): Boolean
    pub fn create_receiver(
        mut env: JNIEnv,
        _this: JClass,
        mirror: *const Transport,
        id: i32,
        adapter: *const Arc<StreamReceiverAdapter>,
    ) -> i32 {
        catcher(&mut env, |_| {
            unsafe { &*mirror }.create_receiver(id as u32, unsafe { &*adapter })?;
            Ok(true)
        })
        .unwrap_or(false) as i32
    }
}
