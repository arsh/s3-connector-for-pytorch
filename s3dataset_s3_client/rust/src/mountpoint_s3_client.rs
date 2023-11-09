use std::sync::Arc;

use mountpoint_s3_client::config::{EndpointConfig, S3ClientAuthConfig, S3ClientConfig};
use mountpoint_s3_client::types::PutObjectParams;
use mountpoint_s3_client::{ObjectClient, S3CrtClient};
use pyo3::types::PyTuple;
use pyo3::{pyclass, pymethods, PyRef, PyResult, ToPyObject};

use crate::exception::python_exception;
use crate::get_object_stream::GetObjectStream;
use crate::list_object_stream::ListObjectStream;
use crate::mountpoint_s3_client_inner::{MountpointS3ClientInner, MountpointS3ClientInnerImpl};
use crate::put_object_stream::PutObjectStream;

#[pyclass(name = "MountpointS3Client", module = "s3dataset_s3_client._s3dataset", frozen)]
pub struct MountpointS3Client {
    client: Arc<dyn MountpointS3ClientInner + Send + Sync + 'static>,

    #[pyo3(get)]
    throughput_target_gbps: f64,
    #[pyo3(get)]
    region: String,
    #[pyo3(get)]
    part_size: usize,
    #[pyo3(get)]
    profile: Option<String>,
    #[pyo3(get)]
    no_sign_request: bool,
}

#[pymethods]
impl MountpointS3Client {
    #[new]
    #[pyo3(signature = (region, throughput_target_gbps=10.0, part_size=8*1024*1024, profile=None, no_sign_request=false))]
    pub fn new_s3_client(
        region: String,
        throughput_target_gbps: f64,
        part_size: usize,
        profile: Option<String>,
        no_sign_request: bool,
    ) -> PyResult<Self> {
        /*
        TODO - Mountpoint has logic for guessing based on instance type.
         It may be worth having similar logic if we want to exceed 10Gbps reading for larger instances
        */

        let endpoint_config = EndpointConfig::new(&region);
        let auth_config = auth_config(profile.as_deref(), no_sign_request);

        let config = S3ClientConfig::new()
            /*
            TODO - Add version number here
             https://github.com/awslabs/mountpoint-s3/blob/73328cc64a2dbca78e879730d4d264aedd881c60/mountpoint-s3/src/main.rs#L427
            */
            .user_agent_prefix("pytorch-loader;mountpoint")
            .throughput_target_gbps(throughput_target_gbps)
            .part_size(part_size)
            .auth_config(auth_config)
            .endpoint_config(endpoint_config);
        let crt_client = Arc::new(S3CrtClient::new(config).map_err(python_exception)?);

        Ok(MountpointS3Client::new(
            region,
            throughput_target_gbps,
            part_size,
            profile,
            no_sign_request,
            crt_client,
        ))
    }

    pub fn get_object(
        slf: PyRef<'_, Self>,
        bucket: String,
        key: String,
    ) -> PyResult<GetObjectStream> {
        slf.client.get_object(slf.py(), bucket, key)
    }

    #[pyo3(signature = (bucket, prefix=String::from(""), delimiter=String::from(""), max_keys=1000))]
    pub fn list_objects(
        &self,
        bucket: String,
        prefix: String,
        delimiter: String,
        max_keys: usize,
    ) -> ListObjectStream {
        ListObjectStream::new(self.client.clone(), bucket, prefix, delimiter, max_keys)
    }

    #[pyo3(signature = (bucket, key, storage_class=None))]
    pub fn put_object(
        slf: PyRef<'_, Self>,
        bucket: String,
        key: String,
        storage_class: Option<String>,
    ) -> PyResult<PutObjectStream> {
        let mut params = PutObjectParams::default();
        params.storage_class = storage_class;

        slf.client.put_object(slf.py(), bucket, key, params)
    }

    pub fn __getnewargs__(slf: PyRef<'_, Self>) -> PyResult<&PyTuple> {
        let py = slf.py();
        let state = [
            slf.region.to_object(py),
            slf.throughput_target_gbps.to_object(py),
            slf.part_size.to_object(py),
            slf.profile.to_object(py),
            slf.no_sign_request.to_object(py),
        ];
        Ok(PyTuple::new(py, state))
    }
}

impl MountpointS3Client {
    pub(crate) fn new<Client: ObjectClient>(
        region: String,
        throughput_target_gbps: f64,
        part_size: usize,
        profile: Option<String>,
        no_sign_request: bool,
        client: Arc<Client>,
    ) -> Self
    where
        Client: Sync + Send + 'static,
        <Client as ObjectClient>::GetObjectResult: Unpin + Sync,
        <Client as ObjectClient>::PutObjectRequest: Sync,
    {
        Self {
            throughput_target_gbps,
            part_size,
            region,
            profile,
            no_sign_request,
            client: Arc::new(MountpointS3ClientInnerImpl::new(client)),
        }
    }
}

fn auth_config(profile: Option<&str>, no_sign_request: bool) -> S3ClientAuthConfig {
    if no_sign_request {
        S3ClientAuthConfig::NoSigning
    } else if let Some(profile_name) = profile {
        S3ClientAuthConfig::Profile(profile_name.to_string())
    } else {
        S3ClientAuthConfig::Default
    }
}